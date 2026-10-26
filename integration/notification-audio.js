/* LR finite notification player, protocol 1. The original OGG stays local.
 * This adapter never creates HTMLAudio or sends audio over the game network.
 */
(function (root) {
  "use strict";
  function create(win) {
    var bridge = win.cef;
    if (!bridge || typeof bridge.notification_sound !== "function") return null;
    var docId = "n" + (win.crypto && win.crypto.randomUUID ? win.crypto.randomUUID() : Date.now().toString(36) + Math.random().toString(36).slice(2));
    var listeners = {}, seq = 0, active = null, timer = null, ready = false, disposed = false;
    var loading = null, readyResolve, readyReject, loadDeadline = 0;
    var player = { ended: false, error: null, currentTime: 0, volume: .35, preload: "auto", src: "" };
    function emit(name) { (listeners[name] || []).slice().forEach(function (fn) { fn(); }); }
    function clearTimer() { if (timer !== null) win.clearTimeout(timer); timer = null; }
    function send(action, token) { bridge.notification_sound(action, token); }
    function fail(message) {
      clearTimer(); player.error = new Error(message);
      if (active) { var old = active; active = null; send("stop", old.token); old.reject(player.error); }
      emit("error");
    }
    function poll() {
      clearTimer();
      if (disposed) return;
      if (!ready && loading) {
        if (Date.now() >= loadDeadline) { readyReject(new Error("Native sound prepare timed out")); fail("Native sound unavailable"); return; }
        send("prepare", docId);
      } else if (active) {
        if (!active.started && Date.now() >= active.deadline) { fail("Native playback start timed out"); return; }
        send("status", active.token);
      }
      if ((!ready && loading) || active) timer = win.setTimeout(poll, 50);
    }
    function response(token, state) {
      if (disposed) return;
      if (token === docId && !ready && loading) {
        if (state === "ready") { ready = true; clearTimer(); readyResolve(); }
        else if (state !== "loading") { readyReject(new Error("Native sound decode failed")); fail("Native sound decode failed"); }
        return;
      }
      if (!active || token !== active.token) return;
      if (state === "playing" || state === "ended") {
        active.started = true;
        active.resolve();
        if (state === "ended") { active = null; clearTimer(); player.ended = true; emit("ended"); }
      } else if (state !== "queued") { fail("Native sound: " + state); }
    }
    bridge.on("lr:notification-sound", response);
    player.addEventListener = function (name, fn) { (listeners[name] || (listeners[name] = [])).push(fn); };
    player.removeEventListener = function (name, fn) { listeners[name] = (listeners[name] || []).filter(function (f) { return f !== fn; }); };
    player.load = function () {
      if (loading || disposed) return;
      loadDeadline = Date.now() + 5000;
      loading = new Promise(function (resolve, reject) { readyResolve = resolve; readyReject = reject; });
      loading.catch(function () {});
      poll();
    };
    player.play = function () {
      if (disposed || player.error || active) return Promise.reject(new Error("Native sound unavailable or busy"));
      player.load(); player.ended = false;
      var request;
      var promise = new Promise(function (resolve, reject) { request = { token: docId + ":" + (++seq), resolve: resolve, reject: reject, started: false, deadline: Date.now()+5000 }; });
      active = request;
      loading.then(function () {
        if (disposed || active !== request) return;
        send("play", request.token); poll();
      }, function (error) { if (active === request) { active = null; request.reject(error); } });
      return promise;
    };
    player.pause = function () {
      if (active) { var old = active; active = null; send("stop", old.token); old.reject(new Error("Native playback cancelled")); }
      clearTimer();
      if (!ready && loading && !player.error) poll();
    };
    function dispose() {
      if (disposed) return;
      disposed = true; player.pause(); clearTimer();
      if (!ready && readyReject) readyReject(new Error("Document closed"));
      if (typeof bridge.off === "function") bridge.off("lr:notification-sound", response);
      win.removeEventListener("pagehide", dispose);
    }
    win.addEventListener("pagehide", dispose);
    return player;
  }
  root.LRNotificationAudio = { create: create };
  if (typeof module !== "undefined") module.exports = { create: create };
})(typeof window !== "undefined" ? window : globalThis);
