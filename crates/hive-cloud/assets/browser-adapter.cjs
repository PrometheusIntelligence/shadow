// .hive-browser-entry.cjs — PLATFORM-GENERATED browser entry
// (browser-auto-generated-entry). Never part of the repository: the build
// writes it into the deployment root when a Node/Bun function has no
// handler-shaped entry of its own, so a tenant never has to ship
// api.browser.js / browser.js / handler.js / index.js / main.js, a package.json
// main/module/exports/scripts.start JS file, or a framework build output.
//
// WHY IT EXISTS. `bundle()`'s gate requires an entry that EXPORTS a
// request→response handler. A framework's real server file is a long-running
// program: it binds a port at import time and exports nothing, so the gate
// rejects it — which is why merely shipping a build output was never enough.
// This adapter is the missing half. It loads whichever built server actually
// exists (the server the build EMBEDDED, then ONE deterministic probe order,
// every attempt guarded so a missing module is a SKIPPED attempt rather than a
// crash) and turns the shape it exports into one call per request.
//
// HOW IT ADAPTS. In order, for the first candidate that yields one:
//   * a callable export — an Express/Connect app, a Next.js/Nuxt/SvelteKit
//     request handler, a bare `(req, res)` function;
//   * an object owning `.handler`, `.handle` or `.app` (Lambda-ish and
//     connect-shaped exports);
//   * an object owning `.fetch` (Hono and Cloudflare-Worker shapes), called
//     with a real `Request` when the substrate has one and answered with a
//     `Response`-shaped result;
//   * the listener a self-starting server handed to `http.createServer(...)`,
//     captured through the stub below — an entry that binds and exports
//     nothing is still adaptable, which is the whole point;
//   * ESM/CJS interop double-wrapping (`mod.default.default`) throughout.
//
// NOTHING HERE BINDS, LISTENS OR FORKS. There is no inbound socket in a donor's
// browser and no host path or port to bind, so `require("http")` is answered
// with a stub: `createServer` captures the listener (the only way to reach an
// app that never exports itself) and every OTHER server method is answered
// HONESTLY rather than as a silent no-op — `listen()` invokes its callback and
// emits `'listening'`, `on()`/`once()`/`off()`/`emit()` are a real (in-memory)
// event registry, and `address()` reports the port the server asked for. A
// server gated on `await new Promise(r => server.listen(0, r))` or a top-level
// `await once(server, 'listening')` must COMPLETE, and a stub that swallowed
// those hung the whole runner.
//
// THE SERVER IS EVALUATED EXACTLY ONCE. `bundle()` embeds this file VERBATIM
// inside `async function (request, ops) { … }`, so its top level re-runs on
// every invocation. The embedded server program is therefore behind a promise
// memoized on `globalThis`, created BEFORE the handler runs: three invocations
// evaluate the server once, not three times (a per-request re-evaluation leaks
// the previous instances' timers, sockets and pools).
//
// HONESTY RULE (unchanged). If no probe adapts, the exported handler THROWS a
// named `HiveBrowserAdapterError` AT THE POINT OF USE naming every candidate it
// tried — never a silent 200 and never a no-op. A module the SUBSTRATE cannot
// provide (a sibling file, `net`, `dns`) fails exactly as it always does: the
// guest's own named error at the call that cannot work.
//
// TRUST RULE (unchanged). This adapter adds NO capability. It uses only what
// the substrate already gives a CommonJS artifact — `require`, `process`,
// `Buffer` — and reaches the host only through `ops.call`, bounded by the
// artifact's `allowed_ops`. It reads no host path, opens no socket, and spawns
// no process.
"use strict";

const __hive_state_base = "__hive_browser_adapter_v1";
const __hive_init_base = "__hive_browser_adapter_init_v1";
const __hive_real_require = typeof require === "function" ? require : null;

// One memo slot per artifact NAME on `globalThis`. It cannot live on the
// exported handler: `bundle()` embeds this whole file inside the
// `async function (request, ops)` it exports, so every invocation re-runs it
// and gets a FRESH handler object — state hung off the function is lost before
// the second request. `globalThis` is the only object that survives, and the
// name in the key keeps two artifacts evaluated into one realm apart (each
// node-worker artifact gets its own Worker, so the common case is one).
function __hive_key(base) {
  const name = typeof __hive_embedded_name === "string" ? __hive_embedded_name : "";
  return base + ":" + name;
}

function __hive_named_error(message) {
  const error = new Error(message);
  error.name = "HiveBrowserAdapterError";
  return error;
}

function __hive_message(error) {
  return String((error && error.message) || error);
}

// Defer a callback to the next microtask — the soonest a listener the server
// registers synchronously after `listen()` can still receive `'listening'`.
function __hive_soon(fn) {
  if (typeof queueMicrotask === "function") {
    queueMicrotask(fn);
  } else if (typeof setTimeout === "function") {
    setTimeout(fn, 0);
  } else {
    fn();
  }
}

// ---------------------------------------------------------------------------
// The `http` stub: capture the listener, and answer every other server method
// the way a real server would.
//
// Declared BEFORE the generated data block on purpose: the embedded server is
// evaluated by that block's init function, so a declaration below it would be
// in its temporal dead zone the moment the server calls `http.createServer`.
// ---------------------------------------------------------------------------
const __hive_captured = { listener: null, server: null };

function __hive_http_stub() {
  const listeners = Object.create(null);
  let address = null;
  let listening = false;
  let listeningDispatched = false;

  function entries(event) {
    if (!Object.prototype.hasOwnProperty.call(listeners, String(event))) listeners[String(event)] = [];
    return listeners[String(event)];
  }
  function detach(event, fn) {
    const list = entries(event);
    for (let i = list.length - 1; i >= 0; i -= 1) if (list[i].fn === fn) list.splice(i, 1);
    return server;
  }

  const server = {
    __hive_stub_server: true,
    listening: false,
    on(event, fn) {
      if (typeof fn !== "function") return server;
      entries(event).push({ fn: fn, once: false });
      // `listen()` already ran: a listener registered now still hears the
      // event, exactly once (a server that awaits `listen` first and only then
      // awaits `once(server, 'listening')` must not wait forever).
      if (String(event) === "listening" && listening && !listeningDispatched) {
        __hive_soon(() => {
          if (!listeningDispatched) server.emit("listening");
        });
      }
      return server;
    },
    once(event, fn) {
      if (typeof fn !== "function") return server;
      entries(event).push({ fn: fn, once: true });
      if (String(event) === "listening" && listening && !listeningDispatched) {
        __hive_soon(() => {
          if (!listeningDispatched) server.emit("listening");
        });
      }
      return server;
    },
    off(event, fn) { return detach(event, fn); },
    removeListener(event, fn) { return detach(event, fn); },
    removeAllListeners(event) {
      if (event === undefined) { for (const key of Object.keys(listeners)) delete listeners[key]; }
      else delete listeners[String(event)];
      return server;
    },
    emit(event, ...args) {
      const list = entries(event).slice();
      if (String(event) === "listening") listeningDispatched = true;
      for (const entry of list) {
        if (entry.once) detach(event, entry.fn);
        entry.fn.apply(server, args);
      }
      return list.length > 0;
    },
    listen(...args) {
      let port = 0;
      for (const arg of args) {
        if (typeof arg === "number" && Number.isFinite(arg)) { port = arg; break; }
        if (typeof arg === "string" && /^\d+$/.test(arg)) { port = Number(arg); break; }
        if (arg && typeof arg === "object" && typeof arg.port !== "undefined") { port = Number(arg.port) || 0; break; }
      }
      const callback = args.find((arg) => typeof arg === "function") || null;
      // A plausible bound address: the server asked for a port and there is no
      // socket, but `server.address().port` is read by half the frameworks in
      // existence and `null` there is a crash, not a graceful degradation.
      address = { port: port > 0 ? port : 3000, address: "127.0.0.1", family: "IPv4" };
      listening = true;
      server.listening = true;
      __hive_soon(() => {
        if (callback) {
          // An error in the server's OWN listen callback is the server's, so
          // it goes to the server's `error` channel (as in Node) rather than
          // escaping as an uncaught exception in the guest.
          try {
            callback();
          } catch (error) {
            server.emit("error", error);
          }
        }
        server.emit("listening");
      });
      return server;
    },
    address() { return address; },
    close(callback) {
      listening = false;
      server.listening = false;
      __hive_soon(() => {
        if (typeof callback === "function") callback();
        server.emit("close");
      });
      return server;
    },
    setTimeout() { return server; },
    ref() { return server; },
    unref() { return server; },
  };

  const stub = {
    createServer(...args) {
      const listener = args.find((arg) => typeof arg === "function") || null;
      if (listener) {
        __hive_captured.listener = listener;
        __hive_captured.server = server;
      }
      return server;
    },
    Server: function __hive_StubServer(...args) {
      return stub.createServer(...args);
    },
    METHODS: ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
    STATUS_CODES: {},
    IncomingMessage: function __hive_StubIncomingMessage() {},
    ServerResponse: function __hive_StubServerResponse() {},
  };
  return stub;
}

// Only `http`/`https` are stubbed; everything else goes to the substrate's own
// loader, so an ordinary `require` behaves exactly as it would in any artifact
// (and a module the substrate lacks throws ITS named error, not a shim lie).
function __hive_require_shim(name) {
  const spec = String(name);
  if (spec === "http" || spec === "node:http" || spec === "https" || spec === "node:https") {
    return __hive_http_stub();
  }
  if (!__hive_real_require) {
    throw __hive_named_error(
      "the embedded server requires " + JSON.stringify(spec) + " but this browser substrate provides no require()"
    );
  }
  return __hive_real_require(spec);
}

/* ---- begin platform-generated data ---- */
const __hive_embedded_name = __HIVE_EMBED_NAME__;
__HIVE_EMBED_BLOCK__
// Every candidate specifier, in the deterministic order they are tried, each
// RELATIVE (`./…`) to the artifact's own directory — the guest filesystem root
// the substrate mounts the artifact under. A BARE specifier (`"server.js"`)
// resolves through `node_modules`, which a one-file artifact never has, so the
// bare form could never resolve even when the file IS present. A relative
// specifier that is not in the mount yields MODULE_NOT_FOUND — a skipped
// attempt, recorded, never a crash.
const __hive_probe_specs = JSON.parse(__HIVE_PROBES_JSON__);
/* ---- end platform-generated data ---- */

// ---------------------------------------------------------------------------
// Shape detection: one export -> one callable handler.
// ---------------------------------------------------------------------------
function __hive_unwrap(value) {
  let v = value;
  // ESM/CJS interop double-wraps the default export.
  for (let i = 0; i < 3 && v && typeof v === "object"; i += 1) {
    if (typeof v.default === "function" || (v.default && typeof v.default === "object")) v = v.default;
    else break;
  }
  return v;
}

function __hive_adapt(name, exported, captured) {
  const v = __hive_unwrap(exported);
  if (typeof v === "function") return { name: name, kind: "node", handler: v };
  if (v && typeof v === "object") {
    if (typeof v.handler === "function") return { name: name + " (.handler)", kind: "node", handler: v.handler.bind(v) };
    if (typeof v.handle === "function") return { name: name + " (.handle)", kind: "node", handler: v.handle.bind(v) };
    if (v.app && typeof v.app === "object" && typeof v.app.handle === "function") {
      return { name: name + " (.app)", kind: "node", handler: v.app.handle.bind(v.app) };
    }
    if (typeof v.fetch === "function") return { name: name + " (.fetch)", kind: "fetch", handler: v.fetch.bind(v) };
  }
  if (typeof captured === "function") return { name: name + " (http listener)", kind: "node", handler: captured };
  return null;
}

// ---------------------------------------------------------------------------
// Synthetic request + collecting response. No socket, no host path, no fork.
// ---------------------------------------------------------------------------
function __hive_decode_base64(text) {
  if (typeof Buffer === "function" || (typeof Buffer === "object" && Buffer && typeof Buffer.from === "function")) {
    try { return Buffer.from(text, "base64").toString("utf8"); } catch (e) { /* fall through */ }
  }
  if (typeof atob === "function") {
    try {
      const raw = atob(text);
      let out = "";
      for (let i = 0; i < raw.length; i += 1) out += String.fromCharCode(raw.charCodeAt(i));
      return decodeURIComponent(escape(out));
    } catch (e) { /* fall through */ }
  }
  return "";
}

function __hive_request(descriptor) {
  const raw = typeof descriptor === "string" ? JSON.parse(descriptor) : (descriptor || {});
  const headers = raw.headers && typeof raw.headers === "object" ? raw.headers : {};
  const path = String(raw.path || "/");
  const queryAt = path.indexOf("?");
  const pathname = queryAt >= 0 ? path.slice(0, queryAt) : path;
  const search = queryAt >= 0 ? path.slice(queryAt) : "";
  const query = {};
  if (search.length > 1) {
    for (const pair of search.slice(1).split("&")) {
      if (!pair) continue;
      const eq = pair.indexOf("=");
      const key = decodeURIComponent((eq >= 0 ? pair.slice(0, eq) : pair).replace(/\+/g, " "));
      const value = eq >= 0 ? decodeURIComponent(pair.slice(eq + 1).replace(/\+/g, " ")) : "";
      query[key] = value;
    }
  }
  let body = typeof raw.body === "string" ? raw.body : "";
  if (!body && typeof raw.bodyBase64 === "string") body = __hive_decode_base64(raw.bodyBase64);
  const contentType = String(headers["content-type"] || headers["Content-Type"] || "");
  let parsed = body;
  if (body && contentType.toLowerCase().indexOf("json") >= 0) {
    try { parsed = JSON.parse(body); } catch (e) { parsed = body; }
  }
  return {
    method: String(raw.method || "GET").toUpperCase(),
    url: path,
    path: pathname,
    pathname: pathname,
    search: search,
    query: query,
    params: {},
    headers: headers,
    body: parsed,
    rawBody: body,
    baseUrl: "",
    originalUrl: path,
    ip: "127.0.0.1",
    __hive_raw: raw,
  };
}

function __hive_as_request(req, encode) {
  if (typeof Request !== "function") {
    throw __hive_named_error(
      "the adapted server exports a fetch-style handler but this browser substrate has no Request constructor"
    );
  }
  const headers = new Headers();
  for (const key of Object.keys(req.headers || {})) {
    try { headers.set(key, String(req.headers[key])); } catch (e) { /* skip an unwritable header */ }
  }
  const method = req.method;
  const init = { method: method, headers: headers };
  const hasBody = method !== "GET" && method !== "HEAD" && req.rawBody;
  if (hasBody) init.body = req.rawBody;
  return new Request("https://browser.invalid" + (req.url || "/"), init);
}

function __hive_response() {
  const state = { status: 200, statusMessage: "", headers: {}, chunks: [], ended: false };
  const encode = (value) => {
    if (typeof value === "string") return value;
    if (value === undefined || value === null) return "";
    if (typeof Buffer === "function" || (typeof Buffer === "object" && Buffer && typeof Buffer.from === "function")) {
      if (value instanceof Uint8Array) return Buffer.from(value).toString("utf8");
    }
    if (value instanceof Uint8Array) return new TextDecoder().decode(value);
    return JSON.stringify(value);
  };
  const res = {
    __hive_response: true,
    __hive_state: state,
    headersSent: false,
    // `statusCode` / `statusMessage` are ACCESSORS over `state`, never plain
    // fields: `res.statusCode = 404` is the single most idiomatic plain-Node
    // way to answer a non-200, and a plain field dropped it on the floor —
    // every such answer came out of the artifact as HTTP 200. `status()`,
    // `writeHead()` and `sendStatus()` write through the same pair, so there
    // is exactly one place the status lives.
    get statusCode() {
      return state.status;
    },
    set statusCode(value) {
      const n = Number(value);
      if (Number.isFinite(n) && n >= 100 && n <= 599) state.status = Math.trunc(n);
    },
    get statusMessage() {
      return state.statusMessage;
    },
    set statusMessage(value) {
      state.statusMessage = value === undefined || value === null ? "" : String(value);
    },
    status(code) { this.statusCode = code; return this; },
    writeHead(code, statusOrHeaders, maybeHeaders) {
      this.statusCode = code;
      let headers = null;
      if (typeof statusOrHeaders === "string") {
        this.statusMessage = statusOrHeaders;
        headers = maybeHeaders;
      } else if (statusOrHeaders && typeof statusOrHeaders === "object") {
        headers = statusOrHeaders;
      }
      if (headers && typeof headers === "object") {
        for (const key of Object.keys(headers)) this.setHeader(key, headers[key]);
      }
      this.headersSent = true;
      return this;
    },
    setHeader(name, value) { state.headers[String(name).toLowerCase()] = String(value); return this; },
    getHeader(name) { return state.headers[String(name).toLowerCase()]; },
    removeHeader(name) { delete state.headers[String(name).toLowerCase()]; return this; },
    set(name, value) { return this.setHeader(name, value); },
    get(name) { return this.getHeader(name); },
    type(value) { return this.setHeader("content-type", value); },
    json(value) { this.setHeader("content-type", "application/json; charset=utf-8"); state.chunks.push(JSON.stringify(value) === undefined ? "null" : JSON.stringify(value)); state.ended = true; this.headersSent = true; return this; },
    send(value) { state.chunks.push(encode(value)); state.ended = true; this.headersSent = true; return this; },
    sendStatus(code) { this.statusCode = code; state.chunks.push(String(state.status)); state.ended = true; this.headersSent = true; return this; },
    write(value) { state.chunks.push(encode(value)); this.headersSent = true; return this; },
    end(value) { if (value !== undefined && value !== null && value !== "") state.chunks.push(encode(value)); state.ended = true; this.headersSent = true; return this; },
    redirect(code, location) {
      this.statusCode = typeof code === "number" ? code : 302;
      this.setHeader("location", typeof code === "string" ? code : String(location || "/"));
      state.ended = true;
      this.headersSent = true;
      return this;
    },
  };
  return res;
}

function __hive_normalize(value, res) {
  // A `Response`-shaped return (fetch-style, or `return res.json(...)` that
  // handed back a real Response).
  if (value && typeof value === "object" && typeof value.status === "number" && typeof value.text === "function") {
    const headers = {};
    if (value.headers && typeof value.headers.forEach === "function") {
      value.headers.forEach((v, k) => { headers[String(k)] = String(v); });
    } else if (value.headers && typeof value.headers === "object") {
      for (const key of Object.keys(value.headers)) headers[key] = String(value.headers[key]);
    }
    return { status: Number(value.status) || 200, headers: headers, body: null, bodyPromise: value.text() };
  }
  if (value && value.__hive_response && value.__hive_state === (res && res.__hive_state)) {
    return { status: value.__hive_state.status, headers: value.__hive_state.headers, body: value.__hive_state.chunks.join(""), bodyPromise: null };
  }
  if (value && value.__hive_response && value.__hive_state) {
    return { status: value.__hive_state.status, headers: value.__hive_state.headers, body: value.__hive_state.chunks.join(""), bodyPromise: null };
  }
  if (typeof value === "string") {
    return { status: res ? res.__hive_state.status : 200, headers: res ? res.__hive_state.headers : {}, body: value, bodyPromise: null };
  }
  if (value && typeof value === "object" && (typeof value.status === "number" || typeof value.statusCode === "number")) {
    const status = Number(value.status !== undefined ? value.status : value.statusCode) || 200;
    const headers = value.headers && typeof value.headers === "object" ? value.headers : {};
    const body = typeof value.body === "string" ? value.body : (value.body === undefined ? "" : JSON.stringify(value.body));
    return { status: status, headers: headers, body: body, bodyPromise: null };
  }
  return null;
}

// ---------------------------------------------------------------------------
// Initialization: evaluate the embedded server EXACTLY ONCE, then adapt it.
//
// The whole of this module re-runs per invocation (`bundle()` embeds it inside
// the `async function (request, ops)` it exports), so the server program is
// behind ONE promise memoized on `globalThis`: the second and later
// invocations reuse the record the first one produced instead of running the
// server again — a per-request re-evaluation leaked each instance's app
// object, timers, sockets and pools while every answer still came from the
// FIRST instance.
// ---------------------------------------------------------------------------
function __hive_init() {
  const key = __hive_key(__hive_init_base);
  if (!globalThis[key]) {
    globalThis[key] = (async () => {
      const record = { name: __hive_embedded_name, embedded: null, captured: null, error: null };
      try {
        if (typeof __hive_embedded_init === "function") {
          record.embedded = await __hive_embedded_init();
          record.captured = __hive_captured.listener;
        }
      } catch (error) {
        record.error = error;
      }
      return record;
    })();
  }
  return globalThis[key];
}

// Resolution: embedded server first, then the require probes. Memoized so one
// worker resolves its server once, not per request.
async function __hive_resolve() {
  const key = __hive_key(__hive_state_base);
  const existing = globalThis[key];
  if (existing) return existing;
  const init = await __hive_init();
  const probed = [];
  const failures = [];
  let resolved = null;

  if (init.error) {
    probed.push(init.name + " (embedded)");
    failures.push(init.name + ": " + __hive_message(init.error));
  } else if (init.embedded !== null && init.embedded !== undefined) {
    probed.push(init.name + " (embedded)");
    try {
      const adapted = __hive_adapt(init.name, init.embedded, init.captured);
      if (adapted) resolved = adapted;
    } catch (error) {
      failures.push(init.name + ": " + __hive_message(error));
    }
  }

  if (!resolved && Array.isArray(__hive_probe_specs) && __hive_real_require) {
    for (const spec of __hive_probe_specs) {
      probed.push(spec);
      let loaded;
      try {
        loaded = __hive_real_require(spec);
      } catch (error) {
        // A missing module (or one the substrate cannot provide) is a SKIPPED
        // attempt, never a crash: keep probing.
        failures.push(spec + ": " + __hive_message(error));
        continue;
      }
      const adapted = __hive_adapt(spec, loaded, null);
      if (adapted) { resolved = adapted; break; }
    }
  }

  if (!resolved) {
    resolved = {
      name: null,
      kind: null,
      handler: null,
      error: __hive_named_error(
        "no adaptable server in this browser artifact — probed [" + probed.join(", ") + "]" +
        (failures.length ? "; failures [" + failures.join(" | ") + "]" : "") +
        ". A browser artifact is ONE self-contained file: a framework server that " +
        "imports siblings or node_modules cannot resolve there. Ship a self-contained " +
        "build output (a bundled/compiled server file), or export the app or handler " +
        "from a single entry file."
      ),
    };
  }
  globalThis[key] = resolved;
  return resolved;
}

// The exported handler: `bundle()`'s gate sees `module.exports` assigned to a
// function, and the artifact envelope calls it as `(request, ops)`.
module.exports = async function __hive_browser_adapter(request, ops) {
  const resolved = await __hive_resolve();
  if (!resolved || !resolved.handler) throw (resolved && resolved.error) || __hive_named_error("no adaptable server");
  const req = __hive_request(request);
  if (resolved.kind === "fetch") {
    const out = await resolved.handler(__hive_as_request(req), {});
    const normalized = __hive_normalize(out, null);
    if (!normalized) throw __hive_named_error("the adapted fetch handler returned no response");
    const body = normalized.body !== null ? normalized.body : await normalized.bodyPromise;
    return { status: normalized.status, headers: normalized.headers, body: body };
  }
  const res = __hive_response();
  const out = await resolved.handler(req, res);
  const normalized = __hive_normalize(out, res) || {
    status: res.__hive_state.status,
    headers: res.__hive_state.headers,
    body: res.__hive_state.chunks.join(""),
    bodyPromise: null,
  };
  const body = normalized.body !== null ? normalized.body : await normalized.bodyPromise;
  if (!normalized.headers["content-type"] && body) normalized.headers["content-type"] = "text/plain; charset=utf-8";
  return { status: normalized.status, headers: normalized.headers, body: body };
};
