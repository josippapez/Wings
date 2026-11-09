import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, cpSync, existsSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const scripts = dirname(new URL(import.meta.url).pathname);

const LOCAL_ONLY = "#!/bin/sh\nprintf '%s\\n' '1) ABC \"Wings Local Signing\"'\n";

function fixture({ arch = "arm64", os = "Darwin", version = "0.1.0", security = LOCAL_ONLY } = {}) {
  const root = mkdtempSync(join(tmpdir(), "wings-native-package-"));
  const native = join(root, "native");
  const bin = join(root, "bin");
  mkdirSync(join(native, "scripts"), { recursive: true });
  mkdirSync(join(root, "desktop", "src-tauri", "icons"), { recursive: true });
  mkdirSync(bin);
  writeFileSync(join(native, "Cargo.toml"), `[package]\nname = "wings-native"\nversion = "${version}"\n`);
  writeFileSync(join(root, "desktop", "src-tauri", "icons", "icon.icns"), "icon");
  writeFileSync(join(root, "test-updater.key"), "test only");
  for (const script of ["build-mac-app.sh", "install-mac-app.sh", "package-mac-app.sh"]) {
    cpSync(join(scripts, script), join(native, "scripts", script));
  }
  writeFileSync(join(bin, "cargo"), `#!/bin/sh
mkdir -p target/release
for binary in wings-native wings-plugin-worker wings wings-terminal-server; do
  printf '#!/bin/sh\\necho %s\\n' "$binary" > "target/release/$binary"
  chmod +x "target/release/$binary"
done
`);
  writeFileSync(join(bin, "security"), security);
  writeFileSync(join(bin, "codesign"), '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALL_LOG"\n');
  writeFileSync(join(bin, "uname"), `#!/bin/sh
if [ "$1" = "-s" ]; then printf '%s\\n' '${os}'; else printf '%s\\n' '${arch}'; fi
`);
  writeFileSync(join(bin, "pnpm"), `#!/bin/sh
for last do :; done
printf 'test-signature' > "$last.sig"
`);
  for (const command of ["open", "osascript", "gh"]) {
    writeFileSync(join(bin, command), `#!/bin/sh\nprintf '%s\\n' '${command}' >> "$CALL_LOG"\nexit 81\n`);
  }
  for (const entry of ["cargo", "security", "codesign", "uname", "pnpm", "open", "osascript", "gh"]) {
    execFileSync("chmod", ["+x", join(bin, entry)]);
  }
  return { root, native, bin, log: join(root, "calls.log") };
}

function run(fixture, env = {}) {
  return spawnSync("sh", ["scripts/package-mac-app.sh"], {
    cwd: fixture.native,
    encoding: "utf8",
    env: { ...process.env, PATH: `${fixture.bin}:${process.env.PATH}`, CALL_LOG: fixture.log, WINGS_UPDATER_KEY: join(fixture.root, "test-updater.key"), ...env },
  });
}

test("packages a signed native bundle and matching local metadata", (t) => {
  const version = "0.2.7";
  const setup = fixture({ version });
  t.after(() => rmSync(setup.root, { recursive: true, force: true }));
  const result = run(setup);
  assert.equal(result.status, 0, result.stderr);

  const output = join(setup.native, "target/release/bundle/macos/darwin-aarch64");
  const archive = `Wings-native-${version}-darwin-aarch64.app.tar.gz`;
  const archivePath = join(output, archive);
  assert.deepEqual(
    execFileSync("tar", ["-tzf", archivePath], { encoding: "utf8" }).trim().split("\n").sort(),
    [
      "Wings-native.app/",
      "Wings-native.app/Contents/",
      "Wings-native.app/Contents/Info.plist",
      "Wings-native.app/Contents/Library/",
      "Wings-native.app/Contents/Library/Wings-native Terminals.app/",
      "Wings-native.app/Contents/Library/Wings-native Terminals.app/Contents/",
      "Wings-native.app/Contents/Library/Wings-native Terminals.app/Contents/Info.plist",
      "Wings-native.app/Contents/Library/Wings-native Terminals.app/Contents/MacOS/",
      "Wings-native.app/Contents/Library/Wings-native Terminals.app/Contents/MacOS/wings-terminal-server",
      "Wings-native.app/Contents/MacOS/",
      "Wings-native.app/Contents/MacOS/wings",
      "Wings-native.app/Contents/MacOS/wings-native",
      "Wings-native.app/Contents/MacOS/wings-plugin-worker",
      "Wings-native.app/Contents/MacOS/wings-terminal-server",
      "Wings-native.app/Contents/Resources/",
      "Wings-native.app/Contents/Resources/icon.icns",
    ],
  );
  const metadata = JSON.parse(readFileSync(join(output, "native-latest-darwin-aarch64.json"), "utf8"));
  assert.deepEqual(metadata, {
    bundle_identifier: "dev.wings.native",
    version,
    platform: "darwin-aarch64",
    artifact: archive,
    sha256: metadata.sha256,
    size: statSync(archivePath).size,
    notes: "",
  });
  assert.equal(readFileSync(join(output, "native-latest-darwin-aarch64.json.sig"), "utf8"), "test-signature");
  assert.equal(metadata.sha256, createHash("sha256").update(readFileSync(archivePath)).digest("hex"));
  const extracted = join(setup.root, "extracted");
  mkdirSync(extracted);
  execFileSync("tar", ["-xzf", archivePath, "-C", extracted]);
  const bundle = join(extracted, "Wings-native.app");
  const plist = readFileSync(join(bundle, "Contents/Info.plist"), "utf8");
  for (const [key, value] of Object.entries({
    CFBundleIdentifier: metadata.bundle_identifier,
    CFBundleExecutable: "wings-native",
    CFBundleShortVersionString: metadata.version,
    CFBundleVersion: metadata.version,
  })) {
    assert.match(plist, new RegExp(`<key>${key}</key><string>${value}</string>`));
  }
  for (const executable of ["wings-native", "wings-plugin-worker", "wings", "wings-terminal-server"]) {
    assert.notEqual(statSync(join(bundle, "Contents/MacOS", executable)).mode & 0o111, 0, `${executable} is executable`);
  }
  // The terminal server's own app, so macOS keys its privacy grants to an app id rather than a file path.
  const helper = join(bundle, "Contents/Library/Wings-native Terminals.app");
  const helperPlist = readFileSync(join(helper, "Contents/Info.plist"), "utf8");
  assert.match(helperPlist, /<key>CFBundleIdentifier<\/key><string>dev\.wings\.native\.terminals<\/string>/);
  assert.notEqual(statSync(join(helper, "Contents/MacOS/wings-terminal-server")).mode & 0o111, 0, "the helper's server is executable");
  assert.equal(
    readFileSync(setup.log, "utf8"),
    "--force --sign Wings Local Signing target/release/Wings-native.app/Contents/Library/Wings-native Terminals.app\n" +
      "--force --deep --sign Wings Local Signing target/release/Wings-native.app\n",
  );
  assert.equal(existsSync(join(setup.root, "Applications")), false);
});

test("signs with a valid Apple Development certificate before the local one, so keychain approvals carry over to new builds", (t) => {
  // `-v` lists only the valid identities, as security(1) does: the local certificate isn't trusted, so it's left out.
  const setup = fixture({
    security: `#!/bin/sh
case "$*" in
  *-v*) printf '%s\\n' '  1) DEF "Apple Development: Someone (TEAM123)"' '     1 valid identities found' ;;
  *) printf '%s\\n' '  1) ABC "Wings Local Signing" (CSSMERR_TP_NOT_TRUSTED)' '  2) DEF "Apple Development: Someone (TEAM123)"' ;;
esac
`,
  });
  t.after(() => rmSync(setup.root, { recursive: true, force: true }));
  const result = run(setup);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    readFileSync(setup.log, "utf8"),
    "--force --sign DEF target/release/Wings-native.app/Contents/Library/Wings-native Terminals.app\n" +
      "--force --deep --sign DEF target/release/Wings-native.app\n",
  );
});

test("maps Intel macOS output and rejects unsupported architectures", (t) => {
  const intel = fixture({ arch: "x86_64" });
  const unsupported = fixture({ arch: "riscv64" });
  t.after(() => {
    rmSync(intel.root, { recursive: true, force: true });
    rmSync(unsupported.root, { recursive: true, force: true });
  });
  assert.equal(run(intel).status, 0);
  assert.equal(existsSync(join(intel.native, "target/release/bundle/macos/darwin-x86_64/Wings-native-0.1.0-darwin-x86_64.app.tar.gz")), true);

  const result = run(unsupported);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Unsupported macOS architecture: riscv64/);
  assert.equal(existsSync(join(unsupported.native, "target/release")), false);
});

test("rejects non-macOS hosts before building", (t) => {
  const setup = fixture({ os: "Linux" });
  t.after(() => rmSync(setup.root, { recursive: true, force: true }));

  const result = run(setup);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Native packaging runs on macOS only/);
  assert.equal(existsSync(join(setup.native, "target/release")), false);
});

test("the installer reuses the native bundle build", () => {
  assert.match(readFileSync(join(scripts, "install-mac-app.sh"), "utf8"), /sh scripts\/build-mac-app\.sh/);
});

test("missing signing key removes stale feeds and never builds or exposes an unsigned feed", (t) => {
  const setup = fixture();
  t.after(() => rmSync(setup.root, { recursive: true, force: true }));
  const output = join(setup.native, "target/release/bundle/macos/darwin-aarch64");
  mkdirSync(output, { recursive: true });
  for (const name of ["native-latest-darwin-aarch64.json", "native-latest-darwin-aarch64.json.sig"]) writeFileSync(join(output, name), "stale");
  rmSync(join(setup.root, "test-updater.key"));
  const result = run(setup);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /No updater key/);
  assert.equal(existsSync(join(output, "native-latest-darwin-aarch64.json")), false);
  assert.equal(existsSync(join(output, "native-latest-darwin-aarch64.json.sig")), false);
  assert.equal(existsSync(join(setup.native, "target/release/wings-native")), false);
});

test("a signing failure leaves no feed or partial signature", (t) => {
  const setup = fixture();
  t.after(() => rmSync(setup.root, { recursive: true, force: true }));
  writeFileSync(join(setup.bin, "pnpm"), "#!/bin/sh\necho 'test signing failure' >&2\nexit 82\n");
  const result = run(setup);
  assert.equal(result.status, 82);
  assert.match(result.stderr, /test signing failure/);
  const output = join(setup.native, "target/release/bundle/macos/darwin-aarch64");
  for (const suffix of ["", ".sig", ".pending", ".pending.sig"]) {
    assert.equal(existsSync(join(output, `native-latest-darwin-aarch64.json${suffix}`)), false);
  }
});

test("WINGS_SWITCH=1 packages Wings.app under the desktop's id and version, with a latest.json for the desktop's updater", (t) => {
  const setup = fixture({ version: "0.1.0" });
  t.after(() => rmSync(setup.root, { recursive: true, force: true }));
  writeFileSync(join(setup.root, "desktop", "src-tauri", "Cargo.toml"), `[package]\nname = "wings"\nversion = "0.2.0"\n`);
  const result = run(setup, { WINGS_SWITCH: "1" });
  assert.equal(result.status, 0, result.stderr);

  const output = join(setup.native, "target/release/bundle/macos/darwin-aarch64");
  const archive = "Wings-0.2.0-darwin-aarch64.app.tar.gz";
  const entries = execFileSync("tar", ["-tzf", join(output, archive)], { encoding: "utf8" }).trim().split("\n");
  assert.ok(entries.every((entry) => entry.startsWith("Wings.app/")), entries.join(", "));
  assert.ok(entries.includes("Wings.app/Contents/MacOS/wings"), "Claude Code's config points at the wings command");
  const feed = JSON.parse(readFileSync(join(output, "wings-latest-darwin-aarch64.json"), "utf8"));
  assert.equal(feed.bundle_identifier, "dev.wings.app");
  assert.equal(feed.version, "0.2.0");
  assert.equal(feed.artifact, archive);
  const latest = JSON.parse(readFileSync(join(output, "latest.json"), "utf8"));
  assert.equal(latest.version, "0.2.0");
  assert.deepEqual(latest.platforms, {
    "darwin-aarch64": { signature: "test-signature", url: `https://github.com/josippapez/Wings/releases/download/v0.2.0/${archive}` },
  });
  const plist = readFileSync(join(setup.native, "target/release/Wings.app/Contents/Info.plist"), "utf8");
  for (const [key, value] of Object.entries({ CFBundleIdentifier: "dev.wings.app", CFBundleName: "Wings", CFBundleExecutable: "wings-native", CFBundleShortVersionString: "0.2.0" })) {
    assert.match(plist, new RegExp(`<key>${key}</key><string>${value}</string>`));
  }
  assert.equal(
    readFileSync(setup.log, "utf8"),
    "--force --sign Wings Local Signing target/release/Wings.app/Contents/Library/Wings Terminals.app\n" +
      "--force --deep --sign Wings Local Signing target/release/Wings.app\n",
  );
  // The side-by-side bundle and its feed are left alone.
  assert.equal(existsSync(join(setup.native, "target/release/Wings-native.app")), false);
  assert.equal(existsSync(join(output, "native-latest-darwin-aarch64.json")), false);
});
