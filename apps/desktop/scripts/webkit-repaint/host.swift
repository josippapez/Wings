// Runs the Wings UI in a real WKWebView and checks that pane badges are drawn after a quick tab switch. The window is
// borderless at desktop level and the app never activates, so it takes no focus. Pixels come from `screencapture -l`,
// which captures what the window server composited, so a layer WebKit forgot to repaint shows as a difference.
// Usage: host <url> <stub.js> <plugin.js> <out dir> <page.html>. Exits 1 when a badge row differs from the reference, or
// when a plugin page is left wrong after the terminal tab in front of it closes.
import AppKit
import WebKit

let args = CommandLine.arguments
let url = URL(string: args[1])!
let stub = try! String(contentsOfFile: args[2], encoding: .utf8)
let pluginJs = try! Data(contentsOf: URL(fileURLWithPath: args[3]))
let out = args[4]
let pageHtml = try! Data(contentsOf: URL(fileURLWithPath: args[5]))

/** Serves the test plugin's page for `.html` and its script for every other `wings-plugin://` request. */
final class PluginScheme: NSObject, WKURLSchemeHandler {
  func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
    let isPage = task.request.url!.path.hasSuffix(".html")
    task.didReceive(HTTPURLResponse(url: task.request.url!, statusCode: 200, httpVersion: nil, headerFields: ["Content-Type": isPage ? "text/html" : "text/javascript", "Access-Control-Allow-Origin": "*"])!)
    task.didReceive(isPage ? pageHtml : pluginJs)
    task.didFinish()
  }
  func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let frame = NSRect(x: 40, y: 40, width: 1400, height: 800)
let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
window.level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.desktopWindow)) + 1)
window.isOpaque = false
window.backgroundColor = .clear
// Like the app's window: a transparent web view over the window's own blur.
let effect = NSVisualEffectView(frame: NSRect(origin: .zero, size: frame.size))
effect.material = .underWindowBackground
effect.blendingMode = .behindWindow
effect.state = .active
window.contentView = effect
let config = WKWebViewConfiguration()
config.setURLSchemeHandler(PluginScheme(), forURLScheme: "wings-plugin")
config.userContentController.addUserScript(WKUserScript(source: stub, injectionTime: .atDocumentStart, forMainFrameOnly: true))
let web = WKWebView(frame: effect.bounds, configuration: config)
web.setValue(false, forKey: "drawsBackground")
// Under other windows the page would count as hidden and stop drawing.
web.perform(Selector(("_setWindowOcclusionDetectionEnabled:")), with: false)
effect.addSubview(web)

func wait(_ seconds: Double) async { try? await Task.sleep(nanoseconds: UInt64(seconds * 1e9)) }
@MainActor func js(_ code: String) async -> Any? { try? await web.evaluateJavaScript(code) }
@MainActor func shot(_ name: String) -> CGImage {
  let path = "\(out)/\(name).png"
  let p = Process()
  p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
  p.arguments = ["-x", "-o", "-l", String(window.windowNumber), path]
  try! p.run()
  p.waitUntilExit()
  let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil)!
  return CGImageSourceCreateImageAtIndex(source, 0, nil)!
}
func pixels(_ image: CGImage) -> [UInt8] {
  var data = [UInt8](repeating: 0, count: image.width * image.height * 4)
  let ctx = CGContext(data: &data, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
  ctx.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
  return data
}
/** Pixels in `rect` (CSS px) that differ by more than a little in any channel. */
func differing(_ a: CGImage, _ b: CGImage, _ rect: [Double]) -> Int {
  let scale = Double(a.width) / frame.width
  let (pa, pb) = (pixels(a), pixels(b))
  var count = 0
  for y in Int(rect[1] * scale)..<Int((rect[1] + rect[3]) * scale) {
    for x in Int(rect[0] * scale)..<Int((rect[0] + rect[2]) * scale) {
      let i = (y * a.width + x) * 4
      if (0..<3).contains(where: { abs(Int(pa[i + $0]) - Int(pb[i + $0])) > 24 }) { count += 1 }
    }
  }
  return count
}

let clickTab = { (i: Int) in "document.querySelectorAll('[data-tab-id] [role=tab]')[\(i)].click()" }
let rowsJs = "JSON.stringify([...document.querySelectorAll('[data-pane-grid]:not(.invisible) [data-pane-key] .justify-end')].map(r => { const b = r.getBoundingClientRect(); return [b.x, b.y, b.width, b.height] }))"

final class Loaded: NSObject, WKNavigationDelegate {
  func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
    Task { @MainActor in
      await wait(5)
      // A tab split in two, then a second tab, then back to the first: the reference.
      _ = await js("document.querySelector('button[aria-label=\"Split right\"]').click()")
      await wait(1.5)
      _ = await js("document.querySelector('button[aria-label=\"New tab\"]').click()")
      await wait(1.5)
      _ = await js(clickTab(0))
      await wait(1)
      let reference = shot("reference")
      let rows = try! JSONSerialization.jsonObject(with: (await js(rowsJs) as! String).data(using: .utf8)!) as! [[Double]]
      guard rows.count == 2, await js("document.querySelectorAll('[data-pane-grid]:not(.invisible) [data-pane-key] .justify-end button').length") as? Int ?? 0 >= 4 else {
        print("setup failed: expected a split tab with badges, found \(rows.count) badge rows")
        exit(2)
      }
      // Quick trips to the other tab and back. The old badge drew half of itself after a stay of 0.15 to 0.3 s.
      var failed = 0
      for (n, stay) in [0.15, 0.2, 0.25, 0.3, 0.3, 0.3].enumerated() {
        _ = await js(clickTab(1))
        await wait(stay)
        _ = await js(clickTab(0))
        await wait(1)
        let image = shot("trip-\(n)")
        let diffs = rows.map { differing(reference, image, $0) }
        let bad = diffs.contains { $0 > 100 }
        if bad { failed += 1 }
        print("\(bad ? "FAIL" : "ok  ") back after \(stay)s: differing pixels per badge row \(diffs)")
      }
      print(failed == 0 ? "Badges drawn after every tab switch." : "\(failed) tab switches left badges undrawn; screenshots in \(out)")

      // A plugin page in its own tab, a terminal tab opened in front of it and closed again. The page must come back
      // as drawn before.
      _ = await js("document.querySelector('button[aria-label=\"New tab menu\"]').click()")
      await wait(0.5)
      _ = await js("[...document.querySelectorAll('[role=menuitem]')].find(e => e.textContent.includes('Issues'))?.click()")
      await wait(3)
      guard let boxJson = await js("(() => { const b = document.querySelector('[data-plugin-tab]')?.getBoundingClientRect(); return b ? JSON.stringify([b.x, b.y, b.width, b.height]) : null })()") as? String,
            let box = try? JSONSerialization.jsonObject(with: boxJson.data(using: .utf8)!) as? [Double] else {
        print("setup failed: the plugin page tab did not open")
        exit(2)
      }
      let pageReference = shot("page-reference")
      var closeFailed = 0
      for (n, stay) in [0.3, 2.0, 6.0].enumerated() {
        _ = await js("document.querySelector('button[aria-label=\"New tab\"]').click()")
        await wait(stay)
        _ = await js("document.querySelector('[data-pane-grid]:not(.invisible) button[aria-label=\"Close pane\"]').click()")
        var elapsed = 0.0
        for after in [0.05, 0.5, 1.5, 3.0] {
          await wait(after - elapsed)
          elapsed = after
          let image = shot("close-\(n)-\(after)")
          let diff = differing(pageReference, image, box)
          if diff > 100 { closeFailed += 1 }
          print("\(diff > 100 ? "FAIL" : "ok  ") page \(after)s after closing a terminal tab opened \(stay)s earlier: \(diff) differing pixels")
        }
      }
      print(closeFailed == 0 ? "Plugin page drawn after every terminal tab closed." : "\(closeFailed) shots of the plugin page differed; screenshots in \(out)")
      exit(failed + closeFailed == 0 ? 0 : 1)
    }
  }
}
let loaded = Loaded()
web.navigationDelegate = loaded
window.orderBack(nil)
web.load(URLRequest(url: url))
DispatchQueue.main.asyncAfter(deadline: .now() + 120) {
  print("timed out")
  exit(2)
}
app.run()
