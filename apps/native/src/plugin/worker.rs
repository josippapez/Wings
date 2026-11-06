use super::protocol::{self, Error, Request, Screen};
use rquickjs::{Context, Function, Persistent, Promise, Runtime, Value};
use std::time::{Duration, Instant};

// debt: reviewed built-in Issues only; add OS sandboxing before loading installed plugins.
const BUILTIN_SOURCE: &str = include_str!("../../../../plugins/tasks/dist/native-issues.js");
const HEAP_BYTES: usize = 64 * 1024 * 1024;
const STACK_BYTES: usize = 256 * 1024;
const REQUEST_DEADLINE: Duration = Duration::from_secs(2);

// Only JSON data crosses into this closure. No Rust function or system capability is registered.
const STORAGE_SETUP: &str = r#"
(input) => {
    const freeze = (value) => {
        if (value !== null && typeof value === 'object') {
            for (const child of Object.values(value)) freeze(child);
            Object.freeze(value);
        }
        return value;
    };
    const project = freeze(input.project);
    const action = input.action;
    const snapshot = new Map(input.storage.map(([key, value]) => [key, freeze(value)]));
    const storage = Object.freeze({
        get: async (key) => {
            if (typeof key !== 'string') throw new TypeError('Storage key must be a string');
            return snapshot.has(key) ? snapshot.get(key) : null;
        },
        entries: async (prefix) => {
            if (typeof prefix !== 'string') throw new TypeError('Storage prefix must be a string');
            return Object.freeze(Array.from(snapshot.entries())
                .filter(([key]) => key.startsWith(prefix))
                .map((entry) => Object.freeze(entry)));
        }
    });
    Object.defineProperty(globalThis, 'wings', {
        value: Object.freeze({ storage }), writable: false, configurable: false
    });
    return async () => globalThis.wingsNative(project, action);
}
"#;

/// One request per process. There is intentionally no source, module, or plugin-path argument.
pub fn run() -> Result<(), Error> {
    let payload = protocol::read_frame(&mut std::io::stdin().lock())?;
    let request: Request = serde_json::from_slice(&payload)?;
    let screen = evaluate(&request, BUILTIN_SOURCE, REQUEST_DEADLINE)?;
    protocol::write_frame(&mut std::io::stdout().lock(), &protocol::encode(&screen)?)
}

fn evaluate(request: &Request, source: &str, budget: Duration) -> Result<Screen, Error> {
    request.validate()?;
    let deadline = Instant::now() + budget;
    let js_error = |_| {
        if Instant::now() >= deadline {
            Error::Deadline
        } else {
            Error::JavaScript
        }
    };
    let runtime = Runtime::new().map_err(js_error)?;
    runtime.set_memory_limit(HEAP_BYTES);
    runtime.set_max_stack_size(STACK_BYTES);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let context = Context::full(&runtime).map_err(js_error)?;
    let input = protocol::encode(request)?;
    let pending = context.with(|ctx| -> Result<_, Error> {
        let input = ctx.json_parse(input).map_err(js_error)?;
        let setup: Function = ctx.eval(STORAGE_SETUP).map_err(js_error)?;
        let invoke: Function = setup.call((input,)).map_err(js_error)?;
        ctx.eval::<(), _>(source).map_err(js_error)?;
        let promise: Promise = invoke.call(()).map_err(js_error)?;
        Ok(Persistent::save(&ctx, promise))
    })?;

    // Drain even after the returned promise settles, so detached microtask chains have the same deadline.
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Deadline);
        }
        if !runtime
            .execute_pending_job()
            .map_err(|_| js_error(rquickjs::Error::Exception))?
        {
            break;
        }
    }

    let screen = context.with(|ctx| -> Result<Screen, Error> {
        let promise = pending.restore(&ctx).map_err(js_error)?;
        let value: Value = promise
            .result()
            .ok_or(Error::Invalid("Plugin requires unsupported async APIs"))?
            .map_err(js_error)?;
        let json = ctx
            .json_stringify(value)
            .map_err(js_error)?
            .ok_or(Error::Invalid("Plugin returned no screen"))?;
        let json = json.to_cstring().map_err(js_error)?;
        if json.len() > protocol::MAX_FRAME_BYTES {
            return Err(Error::Invalid("Plugin response exceeds frame limit"));
        }
        let screen: Screen = serde_json::from_slice(json.as_str().as_bytes())?;
        screen.validate()?;
        Ok(screen)
    })?;
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    Ok(screen)
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{Action, PLUGIN_ID, Project, VERSION};
    use super::*;
    use serde_json::json;

    const SCREEN: &str = r#"({version:1,title:'Issues',state:'empty',rows:[],actions:[{id:'refresh',label:'Refresh'}]})"#;

    fn request() -> Request {
        Request {
            version: VERSION,
            plugin_id: PLUGIN_ID.into(),
            project: Project {
                id: "p".into(),
                name: "Project".into(),
            },
            storage: vec![],
            action: None,
        }
    }

    fn run_js(body: &str) -> Result<Screen, Error> {
        evaluate(
            &request(),
            &format!("globalThis.wingsNative = async (project, action) => {{ {body} }}"),
            REQUEST_DEADLINE,
        )
    }

    #[test]
    fn awaits_promise_jobs_and_passes_project_and_action() {
        let mut input = request();
        input.action = Some(Action::Refresh);
        let source = format!(
            "globalThis.wingsNative = async (project, action) => {{ await Promise.resolve(); await Promise.resolve(); if (project.id !== 'p' || project.name !== 'Project' || action !== 'refresh') throw Error(); return {SCREEN}; }}"
        );
        assert_eq!(
            evaluate(&input, &source, REQUEST_DEADLINE).unwrap().title,
            "Issues"
        );
    }

    #[test]
    fn rejects_throws_rejections_and_unsettled_promises() {
        for source in [
            "throw Error('top level')",
            "globalThis.wingsNative = () => { throw Error('sync') }",
            "globalThis.wingsNative = async () => { throw Error('async') }",
            "globalThis.wingsNative = () => Promise.reject('failure')",
            "globalThis.wingsNative = () => new Promise(() => {})",
            "globalThis.wingsNative = async () => undefined",
        ] {
            assert!(evaluate(&request(), source, REQUEST_DEADLINE).is_err());
        }
    }

    #[test]
    fn browser_system_and_unsupported_apis_are_absent() {
        let body = format!(
            r#"
            for (const key of ['window','document','parent','process','require','fetch','XMLHttpRequest','WebSocket','Deno','Bun','std','os','Worker','console','setTimeout','setInterval']) {{
                if (typeof globalThis[key] !== 'undefined') throw Error(key);
            }}
            if (typeof wings.storage.set !== 'undefined' || typeof wings.storage.delete !== 'undefined') throw Error('write');
            return {SCREEN};
        "#
        );
        run_js(&body).unwrap();
        for body in [
            "await fetch('https://example.com')",
            "await new Promise(resolve => setTimeout(resolve, 0))",
            "require('node:fs')",
            "await import('node:fs')",
        ] {
            assert!(run_js(body).is_err());
        }
    }

    #[test]
    fn storage_is_a_frozen_readonly_snapshot() {
        let mut input = request();
        input.storage = vec![
            ("task:1".into(), json!({"title":"First","nested":{"n":1}})),
            ("other".into(), json!(3)),
        ];
        let source = format!(
            r#"globalThis.wingsNative = async (project) => {{
            const value = await wings.storage.get('task:1');
            const entries = await wings.storage.entries('task:');
            if (value.title !== 'First' || (await wings.storage.get('missing')) !== null) throw Error('get');
            if (entries.length !== 1 || entries[0][0] !== 'task:1') throw Error('entries');
            for (const object of [wings, wings.storage, project, value, value.nested, entries, entries[0]]) {{
                if (!Object.isFrozen(object)) throw Error('mutable');
            }}
            try {{ value.nested.n = 9; }} catch (_) {{}}
            if ((await wings.storage.get('task:1')).nested.n !== 1) throw Error('modified');
            if ('storage' in globalThis || 'input' in globalThis || 'snapshot' in globalThis) throw Error('leaked');
            return {SCREEN};
        }}"#
        );
        evaluate(&input, &source, REQUEST_DEADLINE).unwrap();
    }

    #[test]
    fn interrupts_loops_and_entire_microtask_chain() {
        for source in [
            "while (true) {}".to_owned(),
            "globalThis.wingsNative = async () => { await Promise.resolve(); while (true) {} }".to_owned(),
            "globalThis.wingsNative = () => { const next = () => Promise.resolve().then(next); return next(); }".to_owned(),
            format!("globalThis.wingsNative = async () => {{ const next = () => Promise.resolve().then(next); next(); return {SCREEN}; }}"),
        ] {
            let start = Instant::now();
            assert!(matches!(evaluate(&request(), &source, Duration::from_millis(40)), Err(Error::Deadline)));
            assert!(start.elapsed() < Duration::from_secs(2));
        }
    }

    #[test]
    fn heap_and_stack_are_bounded() {
        assert!(matches!(
            run_js(&format!(
                "new ArrayBuffer(65 * 1024 * 1024); return {SCREEN};"
            )),
            Err(Error::JavaScript)
        ));
        assert!(run_js("const recurse = () => recurse(); recurse();").is_err());
    }

    #[test]
    fn validates_guest_screen_and_response_size() {
        for value in [
            "null",
            "{}",
            "({version:2,title:'Issues',state:'empty',rows:[],actions:[]})",
            "({version:1,title:'Issues',state:'empty',rows:[],actions:[],exec:'x'})",
            "({version:1,title:'x'.repeat(9*1024*1024),state:'empty',rows:[],actions:[]})",
        ] {
            assert!(run_js(&format!("return {value};")).is_err());
        }
    }
}
