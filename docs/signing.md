# Signing Wings on your Mac

macOS ties privacy and keychain approvals to an app's signature. Tauri signs ad hoc by default, and an ad-hoc signature changes with every build, so each new build asks for keychain access again and loses approvals such as Full Disk Access. Signing with one fixed certificate keeps them.

## Make the certificate once

```sh
cat > /tmp/wings-cert.cnf <<'EOF'
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = Wings Local Signing
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
openssl req -x509 -newkey rsa:2048 -nodes -keyout /tmp/wings.key -out /tmp/wings.pem -days 3650 -config /tmp/wings-cert.cnf
PW=$(openssl rand -hex 16)
openssl pkcs12 -export -legacy -inkey /tmp/wings.key -in /tmp/wings.pem -out /tmp/wings.p12 -passout pass:$PW
security import /tmp/wings.p12 -k ~/Library/Keychains/login.keychain-db -P "$PW" -T /usr/bin/codesign
rm /tmp/wings.key /tmp/wings.p12
```

`security find-identity -p codesigning` then lists "Wings Local Signing" as not trusted. That's fine: `codesign` still signs with it.

## Build

```sh
cd apps/desktop && pnpm build:mac
```

It signs with the certificate when your keychain has it, and ad hoc otherwise. `codesign -dr - <Wings.app>` should print `identifier "dev.wings.app" and certificate leaf = H"..."`, which stays the same from build to build.

This only helps on your own Mac. Other Macs need an Apple Developer ID certificate.
