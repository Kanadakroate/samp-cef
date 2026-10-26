const {test} = require('node:test');
const assert = require('node:assert/strict');
const {create} = require('./notification-audio.js');
function fixture() {
  const sent=[], timers=new Map(), events=new Map(); let id=0, reply;
  const win={crypto:{randomUUID:()=> 'test-document'},
    cef:{on:(_,fn)=>reply=fn,off:()=>reply=null,notification_sound:(...v)=>sent.push(v)},
    setTimeout:(fn)=>{timers.set(++id,fn);return id;},clearTimeout:id=>timers.delete(id),
    addEventListener:(name,fn)=>events.set(name,fn),removeEventListener:name=>events.delete(name)};
  const player=create(win);
  return {win,player,sent,timers,events,reply:(...args)=>reply?.(...args),
    ready(){player.load();const token=sent.at(-1)[1];reply(token,'ready');},
    tick(){const f=[...timers.values()];timers.clear();f.forEach(fn=>fn());}};
}
const flush=()=>new Promise(resolve=>setImmediate(resolve));
test('old client remains on explicit compatibility path',()=>assert.equal(create({cef:{}}),null));
test('first and four subsequent sounds prepare once, never overlap, and wait for native end',async()=>{
  const f=fixture(); let ended=0;f.player.addEventListener('ended',()=>ended++);
  f.ready(); await flush();
  for(let i=0;i<5;i++) {
    const p=f.player.play();await flush();const token=f.sent.findLast(v=>v[0]==='play')[1];
    await assert.rejects(f.player.play(),/busy/);
    f.reply(token,'queued'); assert.equal(ended,i);
    f.reply(token,'playing');await p; assert.equal(f.player.ended,false);
    f.reply(token,'ended'); assert.equal(ended,i+1);assert.equal(f.player.ended,true);
    f.reply(token,'ended');assert.equal(ended,i+1);assert.equal(f.timers.size,0);
  }
  assert.equal(f.sent.filter(v=>v[0]==='prepare').length,1);
});
test('cancel while decoding never starts the cancelled sound',async()=>{
  const f=fixture();const p=f.player.play();const rejected=assert.rejects(p,/cancelled/);
  f.player.pause();f.reply('ntest-document','ready');await rejected;await flush();
  assert.equal(f.sent.some(v=>v[0]==='play'),false);
});
test('late end cannot end a newer voice',async()=>{
  const f=fixture();f.ready();const p=f.player.play();const rejected=assert.rejects(p);await flush();
  const old=f.sent.findLast(v=>v[0]==='play')[1];f.player.pause();await rejected;
  const next=f.player.play();await flush();const token=f.sent.findLast(v=>v[0]==='play')[1];
  f.reply(old,'ended');assert.equal(f.player.ended,false);
  f.reply(token,'playing');await next;f.reply(token,'ended');assert.equal(f.player.ended,true);
});
test('decode failure is optional and does not fall back or retry playback',async()=>{
  const f=fixture();let errors=0;f.player.addEventListener('error',()=>errors++);
  const p=f.player.play();const rejected=assert.rejects(p);
  f.reply('ntest-document','error');await rejected;assert.equal(errors,1);assert.equal(f.timers.size,0);
  await assert.rejects(f.player.play());assert.equal(f.sent.some(v=>v[0]==='play'),false);
});
test('document teardown cancels and unregisters without delayed restart',async()=>{
  const f=fixture();f.ready();const p=f.player.play();const rejected=assert.rejects(p);await flush();
  f.events.get('pagehide')();await rejected;f.tick();await flush();
  assert.equal(f.timers.size,0);assert.equal(f.events.has('pagehide'),false);
  assert.equal(f.sent.at(-1)[0],'stop');await assert.rejects(f.player.play());
});
