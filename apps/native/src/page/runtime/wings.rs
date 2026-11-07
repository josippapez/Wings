//! The `wings` object and the globals a frame sees, built from what the host gives it.
//!
//! A bare page has `wings.render` and `setTimeout`/`clearTimeout`. A frame of a plugin, given a `Link` by the host that
//! runs it, has these too, as the desktop's frames do: `wings.broadcast`, `wings.onBroadcast` and `wings.storage`;
//! `setInterval` and `clearInterval`; a `console` that discards what it's given; and `wings.focused` and
//! `wings.onFocus`, which follow the native window's activation state.
//! Those are what `wings-plugin-sdk`'s `channel` and `useQuery` need to run.
//!
//! The rest of the desktop's frame API that a native plugin can use is here too, written as `apps/desktop/public/
//! plugin-sdk.js` writes it: `onPanes`, `setBadge`, `setPaneStatus` and `onAction`, `exec`, `openUrl`, `ports`,
//! `notify`, `transcript`, `onTranscript`, `agentActivity`, `onAgentActivity`, `statusline`, `onTool` and `callTool`, `onHook`, `closeView`, `project`, and
//! `createWorktree`, `worktreeSetup` and `setWorktreeSetup`, `claudeAccounts`, `setClaudeAccount` and `shareClaudeSetup`,
//! `setSidebarLabel`, `fetch`, `secrets`
//! and `post`, `sendToAgent`, `openSettings`, `emit` and `onEvent`, and `viewArgs`. `onReply` is accepted and never called: the
//! native diff viewer has no reply box. `setProjectSection` is there so a plugin gets the host's refusal. `copyText` is
//! the native app's own, for `wings-plugin-ui/native`'s `copyText`: a desktop frame has the browser's clipboard instead.
//! Each call is a promise the host answers through `Link::call`, when it likes and from any thread, which comes back as a
//! message to this frame's loop. Nothing else
//! is added: no files, network or process of the frame's own.
//!
//! The Rust functions are handed to one setup script, which builds the frozen objects from them, so a frame can't reach
//! a raw one except through the object the script made.

use super::{Command, Event, Reply, Services, message, timers::Timers};
use rquickjs::{CatchResultExt, Ctx, Exception, Function, Object, Persistent};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, mpsc},
};

/// What a frame of a plugin reaches through `wings`. The host that runs the frame implements it; each call is made on
/// the frame's own thread, so it must not wait for another frame.
pub trait Link: Send + Sync + 'static {
    fn focused(&self) -> bool {
        true
    }
    fn set_visible(&self, _visible: bool) {}
    /// The app is handing the frame a click, key press or pick of yours.
    fn user_event(&self) {}
    /// The frame broadcast `json`: the host sends it to the plugin's other frames. An error rejects the frame's promise.
    fn broadcast(&self, json: String) -> Result<(), String>;
    /// One call of `wings.storage`. The answer is JSON: the value, a list of keys or of `[key, value]`, or `null`.
    fn storage(&self, call: Store) -> Result<String, String>;
    /// What `wings.openView` opened this page with, as JSON, for `wings.viewArgs`; none for a page opened without args.
    fn view_args(&self) -> Option<String> {
        None
    }
    /// A call of `wings.<method>` with the JSON the frame's wrapper made of its arguments. The host answers once through
    /// `reply`, now or later and from any thread: a call that takes a while must not run on this one.
    fn call(&self, method: &str, params: serde_json::Value, reply: Reply);
}

/// A `wings.storage` call, with the key and value already as the desktop's frame hands them over: `String(key)`, and
/// the JSON text of a value that `JSON.stringify` could make one of.
#[derive(Debug, PartialEq, Eq)]
pub enum Store {
    Get(String),
    Set(String, String),
    Delete(String),
    Keys,
    Entries(String),
}

/// Takes the natives as `native` and defines the globals. Returns the function that hands what the host tells the frame
/// (`deliver(kind, id, text)`: see `runtime::Message`) to its listeners and promises, when the frame has them.
const SETUP: &str = r#"
(native) => {
    const defined = { setTimeout: native.setTimeout, clearTimeout: native.clearTimeout };
    const wings = {};
    if (native.render) wings.render = native.render;
    let deliver;
    if (native.broadcast) {
        const none = () => {};
        const listeners = { broadcast: [], panes: [], transcript: [], agentActivity: [], action: [], projects: [], focus: [], git: [], diffClosed: [], pluginEvent: [], sectionSort: [] };
        let diffRefreshHandler = null;
        let projects = null, focused = native.focused, focusKnown = false;
        const tools = new Map();
        const hooks = new Map();
        // QuickJS has no browser abort globals. Only the signal contract needed by diskFind is supplied.
        class AbortSignal {
            constructor() { this.aborted = false; this.listeners = []; }
            addEventListener(type, fn, options) { if (type === 'abort') this.listeners.push({ fn, once: options?.once }); }
            removeEventListener(type, fn) { if (type === 'abort') this.listeners = this.listeners.filter((entry) => entry.fn !== fn); }
            throwIfAborted() { if (this.aborted) { const error = new Error('The scan was stopped'); error.name = 'AbortError'; throw error; } }
        }
        defined.AbortController = class AbortController {
            constructor() { this.signal = new AbortSignal(); }
            abort() {
                if (this.signal.aborted) return;
                this.signal.aborted = true;
                for (const entry of [...this.signal.listeners]) {
                    if (entry.once) this.signal.removeEventListener('abort', entry.fn);
                    try { entry.fn(); } catch (error) { defined.console.error(error); }
                }
            }
        };
        const subscribe = (list, listener) => {
            list.push(listener);
            return () => {
                const at = list.indexOf(listener);
                if (at >= 0) list.splice(at, 1);
            };
        };
        // Like the desktop's frame: a listener that throws doesn't stop the others, and every listener gets the same copy.
        const hear = (list, data) => {
            for (const listener of [...list]) {
                try { listener(data); } catch (error) { defined.console.error(error); }
            }
        };
        const store = (op, key, json = '') => JSON.parse(native.storage(op, key, json));

        // Calls the host answers: a promise per call, settled by a message to this loop.
        const pending = new Map();
        let nextCall = 1;
        const call = (method, params, onOutput) => new Promise((resolve, reject) => {
            const id = nextCall++;
            pending.set(id, { resolve, reject, onOutput });
            try {
                native.call(id, method, JSON.stringify(params));
            } catch (error) {
                pending.delete(id);
                reject(error);
            }
        });

        // A scan or a search: a call `signal` can stop, that reports progress as JSON lines. Its token names it for `diskUsageCancel`.
        const diskCall = (method, params, { onProgress, signal }) => {
            if (signal?.aborted) return Promise.reject(new Error('The scan was stopped'));
            const token = String(nextCall);
            const stop = () => { call('diskUsageCancel', { token }).catch(none); };
            signal?.addEventListener('abort', stop, { once: true });
            return call(method, { ...params, token, stream: typeof onProgress === 'function' },
                typeof onProgress === 'function' ? (line) => onProgress(JSON.parse(line)) : undefined)
                .finally(() => signal?.removeEventListener('abort', stop));
        };

        // The last pane list, for a listener that registers after it was sent.
        let panes = null;
        // The last badge and status this frame sent for each pane, as JSON, so a repeat of what the host already shows
        // is never sent. Plugins resend on every poll. The host drops a closed pane's, which `forgetPanes` mirrors.
        const sentBadges = new Map();
        const sentStatuses = new Map();
        // The same for each sidebar's label, by sidebar id: it isn't a pane's, so a pane list never clears it.
        const sentLabels = new Map();
        // Sends only when `value` differs from the last one sent under `key`. A call that fails is forgotten, so the next one retries.
        const sendChanged = (sent, key, value, method, params) => {
            const json = JSON.stringify(value);
            if (sent.get(key) === json) return Promise.resolve(null);
            sent.set(key, json);
            return call(method, params).catch((error) => {
                if (sent.get(key) === json) sent.delete(key);
                throw error;
            });
        };
        const forgetPanes = (list) => {
            const live = new Set(Array.isArray(list) ? list.map((pane) => pane?.paneId) : []);
            for (const sent of [sentBadges, sentStatuses]) {
                for (const paneId of sent.keys()) if (!live.has(paneId)) sent.delete(paneId);
            }
        };

        // The host shows a spinner on the clicked button until every action listener has settled.
        const runAction = async (token, data) => {
            let error = null;
            for (const listener of [...listeners.action]) {
                try {
                    await listener(data);
                } catch (e) {
                    error = String(e?.message ?? e);
                    defined.console.error(e);
                }
            }
            call('actionDone', { token, error }).catch(none);
        };

        // Claude or another plugin called one of this plugin's tools. A thrown error goes back to the caller as a failed
        // call. A Claude Code hook (`hook`) answers `{ context?, deny? }` as JSON, and anything that goes wrong says nothing.
        const runTool = async (token, { name, hook, arguments: input, paneId, project, session }) => {
            let text, isError = false;
            const context = { paneId: paneId ?? null, project: project ?? null, session: session ?? null };
            if (hook) {
                let answer = null;
                try {
                    answer = await hooks.get(name)?.(input ?? {}, context);
                } catch (e) {
                    defined.console.error(e);
                }
                const say = typeof answer === 'string' ? { context: answer } : answer && typeof answer === 'object' ? answer : {};
                const out = {};
                if (typeof say.context === 'string' && say.context) out.context = say.context;
                if (typeof say.deny === 'string' && say.deny) out.deny = say.deny;
                return void call('toolDone', { token, text: JSON.stringify(out), isError: false }).catch(none);
            }
            try {
                const handler = tools.get(name);
                if (!handler) throw new Error(`The plugin has no handler for ${name}`);
                const result = await handler(input ?? {}, context);
                text = typeof result === 'string' ? result : result === undefined ? 'Done' : JSON.stringify(result, null, 2);
            } catch (e) {
                text = String(e?.message ?? e);
                isError = true;
            }
            call('toolDone', { token, text, isError }).catch(none);
        };

        deliver = (kind, id, text) => {
            switch (kind) {
                case 'tool': void runTool(id, JSON.parse(text)); break;
                case 'projects': projects = JSON.parse(text); hear(listeners.projects, projects); break;
                case 'focus': {
                    const value = JSON.parse(text).focused === true;
                    if (focusKnown && value === focused) break;
                    focused = value; focusKnown = true;
                    hear(listeners.focus, { focused }); break;
                }
                case 'git': hear(listeners.git, JSON.parse(text)); break;
                case 'diffClosed': hear(listeners.diffClosed, JSON.parse(text)); break;
                case 'pluginEvent': hear(listeners.pluginEvent, JSON.parse(text)); break;
                case 'sectionSort': hear(listeners.sectionSort, JSON.parse(text)); break;
                case 'diffRefresh': {
                    Promise.resolve().then(() => diffRefreshHandler?.(JSON.parse(text))).then(
                        () => call('actionDone', { token: id, error: null }),
                        error => call('actionDone', { token: id, error: String(error?.message ?? error) })
                    ).catch(none);
                    break;
                }
                case 'broadcast': hear(listeners.broadcast, JSON.parse(text)); break;
                case 'panes':
                    panes = JSON.parse(text);
                    forgetPanes(panes);
                    hear(listeners.panes, panes);
                    break;
                case 'transcript': hear(listeners.transcript, JSON.parse(text)); break;
                case 'agentActivity': hear(listeners.agentActivity, JSON.parse(text)); break;
                case 'action': void runAction(id, JSON.parse(text)); break;
                case 'resolve': {
                    const entry = pending.get(id);
                    pending.delete(id);
                    entry?.resolve(JSON.parse(text));
                    break;
                }
                case 'reject': {
                    const entry = pending.get(id);
                    pending.delete(id);
                    entry?.reject(new Error(text));
                    break;
                }
                case 'output':
                    try { pending.get(id)?.onOutput?.(text); } catch (error) { defined.console.error(error); }
                    break;
            }
        };

        Object.assign(wings, {
            // Delivered when this runs, so a frame's messages arrive in the order it sent them.
            broadcast: async (message) => {
                const json = JSON.stringify(message ?? null);
                if (json === undefined) throw new TypeError('A broadcast must be JSON');
                native.broadcast(json);
                return null;
            },
            onBroadcast: (listener) => subscribe(listeners.broadcast, listener),
            storage: Object.freeze({
                get: async (key) => store('get', String(key)),
                set: async (key, value) => {
                    const json = JSON.stringify(value);
                    if (json === undefined) throw new Error('wings.storage.set needs a JSON value. Use delete to remove a key');
                    if (json.length > 1024 * 1024) throw new Error('A stored value is limited to 1 MB');
                    store('set', String(key), json);
                },
                delete: async (key) => void store('delete', String(key)),
                keys: async () => store('keys', ''),
                entries: async (prefix = '') => store('entries', String(prefix)),
            }),
            onFocus: (listener) => {
                const off = subscribe(listeners.focus, listener);
                if (focusKnown) Promise.resolve().then(() => { try { listener({ focused }); } catch (error) { defined.console.error(error); } });
                return off;
            },
            projects: () => call('projects', {}),
            onProjects: (listener) => {
                const off = subscribe(listeners.projects, listener);
                if (projects) Promise.resolve().then(() => { try { listener(projects); } catch (error) { defined.console.error(error); } });
                return off;
            },
            onGitChange: (listener) => subscribe(listeners.git, listener),
            disks: () => call('disks', {}),
            diskUsage: (path, { minBytes, depth, maxEntries, onProgress, signal } = {}) =>
                diskCall('diskUsage', { path, minBytes, depth, maxEntries }, { onProgress, signal }),
            diskFind: (path, { names, skip, onProgress, signal } = {}) =>
                diskCall('diskFind', { path, names, skip }, { onProgress, signal }),
            project: () => call('project', {}),
            focusPane: (paneId) => call('focusPane', { paneId: String(paneId) }),
            tabState: () => call('tabState', {}),
            setTabState: (state) => call('setTabState', { state }),
            blockState: () => call('blockState', {}),
            setBlockState: (state) => call('setBlockState', { state }),
            openPane: ({ command = null, args = [], cwd = null, placement = 'tab', claudeAccount } = {}) => call('openPane', { command, args, cwd, placement, claudeAccount }),
            openView: (id, { as, args, newTab } = {}) => call('openView', { id, as, args, newTab: newTab === true }),
            viewArgs: () => (native.viewArgs == null ? null : JSON.parse(native.viewArgs)),
            openSettings: () => call('openSettings', {}),
            closeView: (id) => call('closeView', { id }),
            onTool: (name, handler) => void tools.set(name, handler),
            onHook: (event, handler) => void hooks.set(event, handler),
            callTool: (name, args, options) => call('callTool', { name, arguments: args ?? {}, project: options?.project ?? null }),
            createWorktree: (options) => call('createWorktree', options),
            worktreeSetup: (projectId) => call('worktreeSetup', { projectId: projectId ?? null }),
            setWorktreeSetup: (projectId, options) => call('setWorktreeSetup', { projectId: projectId ?? null, include: options?.include, setup: options?.setup }),
            claudeAccounts: () => call('claudeAccounts', {}),
            setClaudeAccount: (projectId, name) => call('setClaudeAccount', { projectId, name: name ?? null }),
            shareClaudeSetup: (name, on) => call('shareClaudeSetup', { name, on: on === true }),
            setProjectSection: (projectId, sectionId, rows) => call('setProjectSection', { projectId, sectionId, rows }),
            setSectionSort: (sectionId, sort) => call('setSectionSort', { sectionId, sort }),
            onSectionSort: (listener) => subscribe(listeners.sectionSort, listener),
            openDiff: (diff) => call('openDiff', diff ?? {}),
            updateDiff: (id, update) => call('updateDiff', { id, ...update }),
            onDiffRefresh: (handler) => void (diffRefreshHandler = handler),
            onDiffClosed: (listener) => subscribe(listeners.diffClosed, listener),
            onReply: () => {},
            // Every terminal pane, on every change, and once more for a listener that comes after the first list.
            onPanes: (listener) => {
                const off = subscribe(listeners.panes, listener);
                if (panes) {
                    Promise.resolve().then(() => {
                        try { listener(panes); } catch (error) { defined.console.error(error); }
                    });
                }
                return off;
            },
            onAction: (listener) => subscribe(listeners.action, listener),
            setBadge: (paneId, badge) => sendChanged(sentBadges, paneId, badge, 'setBadge', { paneId, badge }),
            setPaneStatus: (paneId, status) => sendChanged(sentStatuses, paneId, status, 'setPaneStatus', { paneId, status }),
            setSidebarLabel: (sidebarId, label, { rows, tone } = {}) =>
                sendChanged(sentLabels, sidebarId, [label, rows, tone], 'setSidebarLabel', { sidebarId, label, rows, tone }),
            exec: (program, args = [], { cwd = null, timeoutMs = null, onOutput } = {}) =>
                call('exec', {
                    program: String(program),
                    args: Array.isArray(args) ? args.map(String) : [],
                    cwd: typeof cwd === 'string' ? cwd : null,
                    timeoutMs: typeof timeoutMs === 'number' ? timeoutMs : null,
                    stream: typeof onOutput === 'function',
                }, onOutput),
            openUrl: (url) => call('openUrl', { url: String(url) }),
            fetch: (url, { method, headers, body, bearer, binary } = {}) => call('fetch', { url, method, headers, body, bearer, binary }),
            secrets: Object.freeze({
                set: (name, value) => call('secretSet', { name, value }),
                delete: (name) => call('secretDelete', { name }),
                has: (name) => call('secretHas', { name }),
            }),
            post: (program, url, fields, { host } = {}) => call('post', { program, url, fields, host }),
            sendToAgent: (paneId, text) => call('sendToAgent', { paneId, text }),
            emit: (name, data) => call('emit', { name, data: data ?? null }),
            onEvent: (listener) => subscribe(listeners.pluginEvent, listener),
            ports: () => call('ports', {}),
            notify: ({ title, body = '', projectId, actions } = {}) => call('notify', { title, body, projectId, actions }),
            copyText: (text) => call('copyText', { text: String(text) }),
            transcript: (sessionId, types, { last } = {}) =>
                call('transcript', {
                    sessionId: String(sessionId),
                    types: Array.isArray(types) ? types.map(String) : [],
                    last: typeof last === 'number' && last >= 1 ? Math.floor(last) : null,
                }),
            onTranscript: (listener) => subscribe(listeners.transcript, listener),
            agentActivity: (sessionId) => call('agentActivity', { sessionId: String(sessionId) }),
            onAgentActivity: (listener) => subscribe(listeners.agentActivity, listener),
            statusline: () => call('statusline', {}),
        });
        Object.defineProperty(wings, 'focused', { get: () => focused, enumerable: true });
        defined.setInterval = native.setInterval;
        defined.clearInterval = native.clearInterval;
        defined.console = Object.freeze({ log: none, info: none, debug: none, warn: none, error: none });
    }
    defined.wings = Object.freeze(wings);
    // A frame can read these but not swap them for its own.
    for (const [key, value] of Object.entries(defined)) {
        Object.defineProperty(globalThis, key, { value, writable: false, configurable: false, enumerable: true });
    }
    return deliver;
}
"#;

/// Gives the frame its globals. The result is the function that delivers what the host tells the frame, for a frame with a link.
pub(super) fn install<'js>(
    ctx: &Ctx<'js>,
    services: &Services,
    emit: Rc<RefCell<impl FnMut(Event) + 'static>>,
    timers: &Rc<RefCell<Timers>>,
) -> Result<Option<Persistent<Function<'static>>>, String> {
    let native = Object::new(ctx.clone()).map_err(message)?;
    super::timers::install(ctx, &native, timers, services.link.is_some())?;
    if services.render {
        native
            .set("render", render(ctx, emit).map_err(message)?)
            .map_err(message)?;
    }
    if let Some(link) = &services.link {
        native.set("focused", link.focused()).map_err(message)?;
        native
            .set("broadcast", broadcast(ctx, link).map_err(message)?)
            .map_err(message)?;
        native
            .set("storage", storage(ctx, link).map_err(message)?)
            .map_err(message)?;
        if let Some(queue) = &services.queue {
            native
                .set("call", call(ctx, link, queue.clone()).map_err(message)?)
                .map_err(message)?;
        }
        if let Some(args) = link.view_args() {
            native.set("viewArgs", args).map_err(message)?;
        }
    }
    let setup: Function = ctx.eval(SETUP).catch(ctx).map_err(message)?;
    let deliver: Option<Function> = setup.call((native,)).catch(ctx).map_err(message)?;
    Ok(deliver.map(|deliver| Persistent::save(ctx, deliver)))
}

fn render<'js>(
    ctx: &Ctx<'js>,
    emit: Rc<RefCell<impl FnMut(Event) + 'static>>,
) -> rquickjs::Result<Function<'js>> {
    Function::new(ctx.clone(), move |json: String| {
        let event = match super::Tree::parse(&json) {
            Ok(tree) => Event::Tree(tree),
            Err(error) => Event::Error {
                message: error.to_string(),
                fatal: false,
            },
        };
        (emit.borrow_mut())(event);
    })
}

fn broadcast<'js>(ctx: &Ctx<'js>, link: &Arc<dyn Link>) -> rquickjs::Result<Function<'js>> {
    let link = link.clone();
    Function::new(ctx.clone(), move |ctx: Ctx<'js>, json: String| {
        link.broadcast(json)
            .map_err(|error| Exception::throw_message(&ctx, &error))
    })
}

/// `native.call(id, method, json)`: hands the host a call, which it answers later through the frame's queue.
fn call<'js>(
    ctx: &Ctx<'js>,
    link: &Arc<dyn Link>,
    queue: mpsc::SyncSender<Command>,
) -> rquickjs::Result<Function<'js>> {
    let link = link.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, id: u32, method: String, json: String| {
            let params = serde_json::from_str(&json)
                .map_err(|error| Exception::throw_type(&ctx, &error.to_string()))?;
            link.call(
                &method,
                params,
                Reply {
                    call: id,
                    queue: queue.clone(),
                },
            );
            Ok::<(), rquickjs::Error>(())
        },
    )
}

fn storage<'js>(ctx: &Ctx<'js>, link: &Arc<dyn Link>) -> rquickjs::Result<Function<'js>> {
    let link = link.clone();
    Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, op: String, key: String, json: String| {
            let call = match op.as_str() {
                "get" => Store::Get(key),
                "set" => Store::Set(key, json),
                "delete" => Store::Delete(key),
                "keys" => Store::Keys,
                "entries" => Store::Entries(key),
                _ => return Err(Exception::throw_type(&ctx, "unknown storage call")),
            };
            link.storage(call)
                .map_err(|error| Exception::throw_message(&ctx, &error))
        },
    )
}
