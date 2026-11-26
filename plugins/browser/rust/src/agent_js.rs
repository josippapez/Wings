//! The small JavaScript helper the agent tools run in the page, with `eval` in its isolated world (the world shares the DOM but
//! not the page's globals, so a site can't see it or change it). It lives on `window.__wingsAgent` for as long as the page
//! does, so each call sends it again but builds it once. Refs like `e12` are kept in a map of `WeakRef`s, so a page that
//! drops an element doesn't leak it, and a ref to one that is gone says to take a new snapshot.

use serde_json::Value;

/// The most characters a snapshot's outline holds; the rest is cut and says so.
pub const SNAPSHOT_MAX: usize = 12_000;

/// Builds `window.__wingsAgent` once and leaves it in `w`.
const HELPER: &str = r#"var w = window.__wingsAgent || (window.__wingsAgent = (function () {
  var ids = new WeakMap(), refs = {}, next = 1, nodes = 0;
  var HIT = 'a[href],button,input,select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[role=switch],[role=textbox],[role=combobox],[contenteditable=""],[contenteditable=true],[onclick]';
  var DEEPER = /^(section|nav|main|ul|ol|form|header|footer|aside|table|dialog)$/;
  function ref(e) { var id = ids.get(e); if (!id) { id = 'e' + next++; ids.set(e, id); refs[id] = new WeakRef(e); } return id; }
  function get(id) {
    var r = refs[id], e = r && r.deref();
    if (!e || !e.isConnected) throw new Error('Ref ' + id + ' is stale or unknown because the page changed. Take a new snapshot and use its refs.');
    return e;
  }
  function role(e) {
    var r = e.getAttribute('role'), t = e.tagName.toLowerCase(), y = e.type;
    if (r) return r;
    if (t === 'a') return 'link';
    if (t === 'button' || t === 'summary') return 'button';
    if (t === 'select') return 'combobox';
    if (t === 'input') return y === 'checkbox' || y === 'radio' ? y : /^(button|submit|reset|image)$/.test(y) ? 'button' : 'textbox';
    return 'textbox';
  }
  function name(e) {
    var l = e.labels && e.labels[0];
    var t = e.getAttribute('aria-label') || (l && l.innerText) || e.placeholder || e.alt || e.title || (/^(button|submit|reset)$/.test(e.type) ? e.value : '') || e.innerText || '';
    return t.replace(/\s+/g, ' ').trim().slice(0, 80);
  }
  function snapshot(max) {
    var out = [], size = 0;
    function line(d, s) { size += s.length + 1; if (size <= max) out.push('  '.repeat(Math.min(d, 8)) + s); }
    function walk(e, d) {
      if (size > max || e.hidden || e.getAttribute('aria-hidden') === 'true') return;
      var st = getComputedStyle(e), t = e.tagName.toLowerCase();
      if (st.display === 'none' || st.visibility === 'hidden' || /^(script|style|noscript|svg|template)$/.test(t)) return;
      if (e.matches(HIT)) {
        var r = role(e), n = name(e), s = '- ' + r + ' [' + ref(e) + ']' + (n ? ' "' + n + '"' : '');
        if (e.href) s += ' -> ' + e.getAttribute('href').slice(0, 100);
        if (/^(textbox|combobox)$/.test(r) && e.value) s += ' value="' + String(e.value).slice(0, 60) + '"';
        if (e.checked) s += ' checked';
        if (e.disabled) s += ' disabled';
        return line(d, s);
      }
      var own = Array.prototype.filter.call(e.childNodes, function (c) { return c.nodeType === 3; }).map(function (c) { return c.data; }).join(' ').replace(/\s+/g, ' ').trim();
      if (/^h[1-6]$/.test(t)) line(d, '# ' + (e.innerText || '').replace(/\s+/g, ' ').trim().slice(0, 120));
      else if (own) line(d, own.slice(0, 200));
      var kids = (e.shadowRoot || e).children, deeper = d + (DEEPER.test(t) ? 1 : 0);
      for (var i = 0; i < kids.length && size <= max; i++) walk(kids[i], deeper);
    }
    walk(document.body, 0);
    return 'Page: ' + document.title + '\nURL: ' + location.href + '\n' + out.join('\n') + (size > max ? '\n... cut at ' + max + ' characters: scroll, or use wait_for and eval to read the rest' : '');
  }
  function at(e) { var b = e.getBoundingClientRect(); return { bubbles: true, cancelable: true, view: window, clientX: b.x + b.width / 2, clientY: b.y + b.height / 2 }; }
  function fire(e, types, o) { types.forEach(function (t) { e.dispatchEvent(new (t.indexOf('pointer') === 0 ? PointerEvent : MouseEvent)(t, o)); }); }
  function setv(e, v) {
    var p = e instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : e instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(p, 'value').set.call(e, v);
  }
  function input(e, extra) { e.dispatchEvent(new InputEvent('input', Object.assign({ bubbles: true }, extra))); e.dispatchEvent(new Event('change', { bubbles: true })); }
  return {
    snapshot: snapshot,
    has: function (q) { return !!((q.text && document.body.innerText.indexOf(q.text) >= 0) || (q.selector && document.querySelector(q.selector))); },
    click: function (id) {
      var e = get(id); e.scrollIntoView({ block: 'center', inline: 'center' });
      var o = at(e); fire(e, ['pointerdown', 'mousedown'], o); e.focus && e.focus(); fire(e, ['pointerup', 'mouseup'], o); e.click();
      return 'Clicked ' + id;
    },
    fill: function (id, v) {
      var e = get(id); e.focus();
      if (e.isContentEditable) e.textContent = v;
      else if ('value' in e && !/^(checkbox|radio|file)$/.test(e.type)) setv(e, v);
      else throw new Error(id + ' is not a text field: use click for checkboxes, radios and buttons');
      input(e, { data: v, inputType: 'insertText' });
      return 'Filled ' + id;
    },
    type: function (t) {
      var e = document.activeElement;
      if (!e || e === document.body) throw new Error('Nothing has focus: click or fill a field first');
      if (e.isContentEditable) document.execCommand('insertText', false, t);
      else {
        var s = e.selectionStart;
        if (s == null) setv(e, e.value + t); else { setv(e, e.value.slice(0, s) + t + e.value.slice(e.selectionEnd)); e.setSelectionRange(s + t.length, s + t.length); }
        input(e, { data: t, inputType: 'insertText' });
      }
      return 'Typed ' + t.length + ' characters';
    },
    press: function (combo) {
      var p = combo.split('+'), k = p.pop() || '+', m = p.map(function (x) { return x.toLowerCase(); }), has = function () { return Array.prototype.some.call(arguments, function (x) { return m.indexOf(x) >= 0; }); };
      if (k === 'Space') k = ' ';
      var o = { key: k, code: k.length === 1 ? (/[a-z]/i.test(k) ? 'Key' + k.toUpperCase() : /\d/.test(k) ? 'Digit' + k : k) : k, ctrlKey: has('control', 'ctrl'), shiftKey: has('shift'), altKey: has('alt', 'option'), metaKey: has('meta', 'cmd', 'command'), bubbles: true, cancelable: true };
      var e = document.activeElement || document.body;
      // A synthetic Enter doesn't submit a form by itself, as a real one does.
      if (e.dispatchEvent(new KeyboardEvent('keydown', o)) && k === 'Enter' && e.tagName === 'INPUT' && e.form) e.form.requestSubmit();
      e.dispatchEvent(new KeyboardEvent('keyup', o));
      return 'Pressed ' + combo;
    },
    hover: function (id) { var e = get(id); e.scrollIntoView({ block: 'center' }); fire(e, ['pointerover', 'mouseover', 'pointermove', 'mousemove'], at(e)); return 'Hovered ' + id; },
    select: function (id, wanted) {
      var e = get(id), hit = 0;
      if (e.tagName !== 'SELECT') throw new Error(id + ' is not a select element: use click');
      Array.prototype.forEach.call(e.options, function (o) { o.selected = wanted.indexOf(o.value) >= 0 || wanted.indexOf(o.text.trim()) >= 0; hit += o.selected ? 1 : 0; });
      if (!hit) throw new Error('No option of ' + id + ' has that value or label: ' + Array.prototype.map.call(e.options, function (o) { return o.text.trim(); }).slice(0, 20).join(', '));
      input(e);
      return 'Selected ' + hit + ' option(s) of ' + id;
    },
    scroll: function (a) {
      var e = a.ref ? get(a.ref) : null;
      if (e) e.scrollIntoView({ block: 'center' });
      else if (a.to === 'top') scrollTo(0, 0);
      else if (a.to === 'bottom') scrollTo(0, document.documentElement.scrollHeight);
      else scrollBy(a.x || 0, a.y || 0);
      return 'Scrolled to ' + Math.round(scrollY) + ' of ' + document.documentElement.scrollHeight + ' px';
    },
    drag: function (from, to) {
      var a = get(from), b = get(to), dt = new DataTransfer(), o = at(a);
      a.scrollIntoView({ block: 'center' });
      fire(a, ['pointerdown', 'mousedown'], o);
      var d = function (e, t) { e.dispatchEvent(new DragEvent(t, Object.assign(at(e), { dataTransfer: dt }))); };
      d(a, 'dragstart'); d(b, 'dragenter'); d(b, 'dragover'); d(b, 'drop'); d(a, 'dragend');
      fire(b, ['pointerup', 'mouseup'], at(b));
      return 'Dragged ' + from + ' to ' + to;
    }
  };
})());
"#;

/// The body of an async function for `eval` that calls `w.<func>(<args>)` and returns what it gives. The arguments are
/// JSON, which is valid JavaScript, so nothing the agent sent can end the call early.
pub fn body(func: &str, args: &[Value]) -> String {
    let args: Vec<String> = args.iter().map(Value::to_string).collect();
    format!("{HELPER}return w.{func}({});", args.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_helper_is_small_and_balanced_and_has_every_function_the_tools_call() {
        assert!(HELPER.len() < 9_000, "{} bytes", HELPER.len());
        for (open, close) in [('(', ')'), ('{', '}'), ('[', ']')] {
            // Brackets in the regexes and strings are few and paired, so the counts match.
            assert_eq!(HELPER.matches(open).count(), HELPER.matches(close).count(), "{open}{close}");
        }
        for func in ["snapshot:", "has:", "click:", "fill:", "type:", "press:", "hover:", "select:", "scroll:", "drag:"] {
            assert!(HELPER.contains(func), "{func}");
        }
        // It runs in an isolated world and leaves one global, and nothing in it loads or sends anything.
        assert_eq!(HELPER.matches("window.__wingsAgent").count(), 2);
        assert!(!HELPER.contains("fetch(") && !HELPER.contains("XMLHttpRequest") && !HELPER.contains("eval("));
    }

    #[test]
    fn a_call_is_the_helper_then_a_return_with_the_arguments_as_json() {
        let script = body("fill", &[json!("e3"), json!("a\"); alert(1); (\"")]);
        assert!(script.starts_with("var w = window.__wingsAgent"));
        assert!(script.ends_with(r#"return w.fill("e3", "a\"); alert(1); (\"");"#));
        assert!(body("snapshot", &[json!(SNAPSHOT_MAX)]).ends_with("return w.snapshot(12000);"));
    }
}
