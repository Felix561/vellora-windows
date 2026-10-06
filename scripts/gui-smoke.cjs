/*
 * Offline GUI smoke test. Requires Node 22.22.2+, `npm run dev -- --host 127.0.0.1`,
 * and a separate Chrome/Edge test profile with CDP on localhost:19329.
 * Example browser flags (run in a hidden process):
 *   --headless=new --remote-debugging-port=19329 --user-data-dir=<temporary test profile>
 * Run: node scripts/gui-smoke.cjs
 * Validate preload isolation without a browser: node scripts/gui-smoke.cjs --self-test
 * This creates a fresh test tab, supplies only synthetic data through mocked
 * Tauri IPC, performs no OpenAI calls, and saves screenshots in the OS temp folder.
 * It does not touch the installed application's settings, history, or clipboard.
 * CI uses .github/scripts/run-gui-smoke.ps1 on an isolated hosted Windows runner.
 * Display scaling is CDP viewport/device-pixel emulation, not proof of native
 * Windows DPI handling or a user's browser zoom settings.
 */
const fs = require('node:fs');
const os = require('node:os');
const assert = require('node:assert/strict');
const path = require('node:path');
const projectRoot = path.resolve(__dirname, '..');
const { version: appVersion } = JSON.parse(fs.readFileSync(path.join(projectRoot, 'package.json'), 'utf8'));
assert.match(appVersion, /^\d+\.\d+\.\d+(?:-[\w.-]+)?$/, 'Package version must be suitable for synthetic support information');
const mock = `(() => {
  const callbacks = new Map(), listeners = new Map(); let seq = 0;
  const sample = {id:'audit-example',createdAt:'2026-10-01T14:00:00Z',provider:'openai',sttModel:'gpt-4o-mini-transcribe',cleanupMode:'off',rawText:'A sample transcript for interface testing.',finalText:'A sample transcript for interface testing.',wordCount:7,charCount:42,pasted:false,error:'The destination application rejected automatic paste.'};
  const settings = {appearance:'classic',provider:'openai',sttModel:'gpt-4o-mini-transcribe',cleanupEnabled:false,cleanupMode:'light',cleanupModel:'gpt-5-nano-2025-08-07',autoPaste:true,copyToClipboard:true,saveHistory:true,language:'auto',hotkey:'Ctrl+Win'};
  const cost = {pricingVersion:'audit-fixture',estimatedTotalUsd:0.012,estimatedTodayUsd:0.006,estimatedWeekUsd:0.012,estimatedTranscriptionUsd:0.012,estimatedCleanupUsd:0,billableAudioSeconds:240,measuredAudioSeconds:240,estimatedAudioSeconds:0,estimatedFromWordsCount:0,unknownModelCount:0,week:Array.from({length:7},(_,i)=>({date:'2026-09-'+(24+i),dayLabel:['Mon','Tue','Wed','Thu','Fri','Sat','Sun'][i],estimatedCostUsd:0.0012,transcriptionCount:1,audioSeconds:30})),modelBreakdown:[{model:'gpt-4o-mini-transcribe',estimatedCostUsd:0.012,transcriptionCount:1,audioSeconds:240}],note:'Estimated from local history. Not official billing data.'};
  window.__audit = {settings,history:[sample],snapshot:{state:'error',message:sample.error,lastTranscript:sample},fail:null,calls:[],unhandled:[],networkAttempts:[],emit:(name,payload)=>{for(const id of listeners.get(name)||[]) callbacks.get(id)?.({event:name,payload});}};
  // Stop page networking before initiation, including WebSockets (which use
  // dedicated CDP events). HTTP asset loads also pass a CDP Fetch allowlist.
  const assertLoopback = (input, websocket=false) => {
    const url=new URL(typeof input==='string'||input instanceof URL?input:input.url,location.href);
    if(url.origin!==(websocket?'ws://127.0.0.1:1420':'http://127.0.0.1:1420')){
      window.__audit.networkAttempts.push(true);
      throw Error('External network access blocked by the synthetic test fixture');
    }
    return url;
  };
  if(typeof window.fetch==='function'){
    const nativeFetch=window.fetch.bind(window);
    window.fetch=(input,options)=>{assertLoopback(input);return nativeFetch(input,options);};
  }
  if(typeof window.XMLHttpRequest==='function'){
    const nativeOpen=window.XMLHttpRequest.prototype.open;
    window.XMLHttpRequest.prototype.open=function(method,url,...rest){assertLoopback(url);return nativeOpen.call(this,method,url,...rest);};
  }
  if(typeof window.WebSocket==='function'){
    const NativeWebSocket=window.WebSocket;
    window.WebSocket=class extends NativeWebSocket{constructor(url,protocols){assertLoopback(url,true);super(url,protocols);}};
  }
  if(typeof window.EventSource==='function'){
    const NativeEventSource=window.EventSource;
    window.EventSource=class extends NativeEventSource{constructor(url,options){assertLoopback(url);super(url,options);}};
  }
  Object.defineProperty(navigator,'sendBeacon',{configurable:true,value:(url)=>{assertLoopback(url);return false;}});
  const blockWorker=()=>{window.__audit.networkAttempts.push(true);throw Error('Workers are disabled in the synthetic test fixture');};
  if(typeof window.Worker==='function')window.Worker=class{constructor(){blockWorker();}};
  if(typeof window.SharedWorker==='function')window.SharedWorker=class{constructor(){blockWorker();}};
  if(navigator.serviceWorker)Object.defineProperty(navigator.serviceWorker,'register',{configurable:true,value:blockWorker});
  // Help's support export uses the browser Clipboard API rather than Tauri IPC.
  // Stub it too: synthetic tests must not read or replace the real OS clipboard.
  Object.defineProperty(navigator, 'clipboard', {configurable:true,value:{
    writeText:async text=>{window.__audit.supportClipboard=String(text);},
    readText:async ()=>window.__audit.supportClipboard??''
  }});
  window.addEventListener('unhandledrejection',e=>window.__audit.unhandled.push(String(e.reason)));
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {unregisterListener:(event,id)=>{listeners.set(event,(listeners.get(event)||[]).filter(x=>x!==id));callbacks.delete(id)}};
  window.__TAURI_INTERNALS__ = {metadata:{currentWindow:{label:'main'},currentWebview:{label:'main',windowLabel:'main'}},transformCallback:(fn)=>{callbacks.set(++seq,fn);return seq},unregisterCallback:(id)=>callbacks.delete(id),invoke:async (cmd,args={})=>{
    const a=window.__audit; a.calls.push({cmd,args});
    if(cmd==='plugin:event|listen'){listeners.set(args.event,[...(listeners.get(args.event)||[]),args.handler]);return args.handler;}
    if(cmd==='plugin:event|unlisten')return;
    if(a.fail===cmd)throw Error('Simulated '+cmd+' failure');
    if(cmd==='get_settings')return structuredClone(a.settings);
    if(cmd==='save_settings'){a.settings=structuredClone(args.settings);a.emit('settings-changed',structuredClone(a.settings));return structuredClone(a.settings);}
    if(cmd==='has_openai_api_key')return true;
    if(cmd==='get_dictation_snapshot')return structuredClone(a.snapshot);
    if(cmd==='get_app_health')return {ready:true,startupPhase:'ready',backendStartedAt:'2026-10-01T14:00:00Z',singleInstance:true};
    if(cmd==='get_support_info')return {version:${JSON.stringify(appVersion)},platform:'windows',architecture:'x86_64',startupPhase:'ready',ready:true,singleInstance:true,keyConfigured:true,microphoneAvailable:true,dictationState:a.snapshot.state,deviceName:'Private device name',rawText:'Private transcript text'};
    if(cmd==='check_microphone')return {available:true,deviceName:'Synthetic microphone'};
    if(cmd==='list_transcripts')return structuredClone(a.history);
    if(cmd==='get_start_at_login'||cmd==='set_start_at_login')return args.enabled??false;
    if(cmd==='get_dashboard_stats')return {totalWords:a.history.length*7,totalTranscripts:a.history.length,transcriptsToday:a.history.length,wordsToday:a.history.length*7,estimatedMinutesSaved:1,averageWordsPerTranscript:7,mostActiveDay:'Thu',week:cost.week.map(d=>({...d,transcriptCount:1,wordCount:7})),modelUsage:[],cost};
    if(cmd==='delete_transcript'){a.history=a.history.filter(x=>x.id!==args.id);a.snapshot.lastTranscript=null;a.emit('history-changed');return true;}
    if(cmd==='clear_transcripts'){const n=a.history.length;a.history=[];a.snapshot.lastTranscript=null;a.emit('history-changed');return n;}
    if(['save_openai_api_key','delete_openai_api_key','copy_transcript','paste_transcript','open_help_link'].includes(cmd))return null;
    throw Error('Unmocked Tauri command: '+cmd);
  }};
})()`;
const makeKeyEvent=(name,shift=false)=>{
  const codes={Tab:9,ArrowRight:39,Home:36,Enter:13};
  assert(Object.hasOwn(codes,name),'Smoke input must use a supported key');
  // CDP modifier bit field: Alt=1, Ctrl=2, Meta=4, Shift=8.
  // https://raw.githubusercontent.com/ChromeDevTools/devtools-protocol/master/pdl/domains/Input.pdl
  return {key:name,code:name,windowsVirtualKeyCode:codes[name],nativeVirtualKeyCode:codes[name],modifiers:shift?8:0};
};
(async()=>{
  if(process.argv.includes('--self-test')){
    assert.equal(makeKeyEvent('Tab').modifiers,0,'Plain Tab must not carry modifiers');
    assert.equal(makeKeyEvent('Tab',true).modifiers,8,'Shift+Tab must carry the CDP Shift bit rather than Alt');
    assert.equal(makeKeyEvent('Tab',true).windowsVirtualKeyCode,9,'Tab keeps its virtual key code when Shift is held');
    const vm=require('node:vm');
    const isolated={window:{addEventListener(){},fetch:async()=>({synthetic:true}),WebSocket:class{}},navigator:{},location:{href:'http://127.0.0.1:1420/'},URL,structuredClone};
    vm.runInNewContext(mock,isolated,{timeout:1000});
    const internals=isolated.window.__TAURI_INTERNALS__;
    assert.equal((await internals.invoke('get_support_info')).version,appVersion,'Synthetic support version follows package.json');
    await isolated.navigator.clipboard.writeText('Synthetic self-test support text');
    assert.equal(await isolated.navigator.clipboard.readText(),'Synthetic self-test support text','Clipboard is isolated in the fixture');
    await assert.rejects(internals.invoke('unmocked_self_test'),/Unmocked Tauri command/,'Unknown commands fail closed');
    isolated.window.__audit.fail='save_settings';
    await assert.rejects(internals.invoke('save_settings',{settings:{}}),/Simulated save_settings failure/,'Fixture failures remain testable');
    assert.equal((await isolated.window.fetch('http://127.0.0.1:1420/synthetic')).synthetic,true,'Loopback fetch stays available');
    assert.throws(()=>isolated.window.fetch('http://example.invalid/'),/External network access blocked/,'External HTTP must be blocked before calling fetch');
    assert.throws(()=>new isolated.window.WebSocket('wss://example.invalid/'),/External network access blocked/,'External WebSockets must be blocked before construction');
    assert.doesNotThrow(()=>new isolated.window.WebSocket('ws://127.0.0.1:1420/'),'Loopback Vite WebSocket stays available');
    console.log(`PASS: Vellora ${appVersion} GUI input/preload/version/clipboard/network isolation self-test; no browser or application accessed.`);
    return;
  }
  const target=await(await fetch('http://127.0.0.1:19329/json/new?about:blank',{method:'PUT'})).json();
  const ws=new WebSocket(target.webSocketDebuggerUrl); await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j});
  const pending=new Map();let seq=0;
  const externalRequests=[];
  const interceptionErrors=[];
  const allowedRequest=url=>{try{return new URL(url).origin==='http://127.0.0.1:1420';}catch{return false;}};
  ws.onmessage=e=>{
    const m=JSON.parse(e.data);
    if(m.method==='Network.requestWillBeSent'&&!allowedRequest(m.params.request.url))externalRequests.push(true);
    if(m.method==='Network.webSocketCreated'&&new URL(m.params.url).origin!=='ws://127.0.0.1:1420')externalRequests.push(true);
    if(m.method==='Fetch.requestPaused'){
      const allowed=allowedRequest(m.params.request.url);
      if(!allowed)externalRequests.push(true);
      void send(allowed?'Fetch.continueRequest':'Fetch.failRequest',allowed?{requestId:m.params.requestId}:{requestId:m.params.requestId,errorReason:'BlockedByClient'}).catch(()=>interceptionErrors.push(true));
    }
    if(pending.has(m.id)){pending.get(m.id)(m);pending.delete(m.id)}
  };
  const send=async(method,params={})=>{const id=++seq;let timeout;const p=new Promise((resolve,reject)=>{pending.set(id,resolve);timeout=setTimeout(()=>{pending.delete(id);reject(Error(`CDP command timed out: ${method}`));},10000);});ws.send(JSON.stringify({id,method,params}));let r;try{r=await p;}finally{clearTimeout(timeout);}if(r.error)throw Error(`CDP command failed: ${method}`);return r.result;};
  const ev=async(expression)=>{const r=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw Error(JSON.stringify(r.exceptionDetails));return r.result.value};
  const sleep=ms=>new Promise(r=>setTimeout(r,ms));
  const waitFor=async(exp)=>{for(let i=0;i<200;i++){if(await ev(exp))return;await sleep(100)}throw Error('Timed out: '+exp);};
  const click=async(text)=>{await ev(`(()=>{const b=Array.from(document.querySelectorAll('button')).find(x=>x.textContent.trim()===${JSON.stringify(text)});if(!b)throw Error('Button missing: '+${JSON.stringify(text)});b.click()})()`);await sleep(100);};
  const nav=async(text)=>{await ev(`Array.from(document.querySelectorAll('.nav button')).find(x=>x.textContent.trim()===${JSON.stringify(text)}).click()`);await sleep(150);};
  // Dispatch through Chromium's input pipeline. A DOM KeyboardEvent alone does
  // not exercise browser tab order or the :focus-visible keyboard heuristic.
  const key=async(name,shift=false)=>{
    const event=makeKeyEvent(name,shift);
    await send('Input.dispatchKeyEvent',{type:'rawKeyDown',...event});
    await send('Input.dispatchKeyEvent',{type:'keyUp',...event});
    await sleep(50);
  };
  const assertFocus=async(expected,label)=>{
    const actual=await ev(`(()=>{const el=document.activeElement;const rect=el.getBoundingClientRect();const style=getComputedStyle(el);return {expected:el.matches(${JSON.stringify(expected)}),focusVisible:el.matches(':focus-visible'),outlineWidth:parseFloat(style.outlineWidth),outlineStyle:style.outlineStyle,visible:rect.width>0&&rect.height>0&&rect.right>0&&rect.left<innerWidth&&rect.bottom>0&&rect.top<innerHeight};})()`);
    assert(actual.expected,`${label}: keyboard focus should reach the intended control`);
    assert(actual.focusVisible,`${label}: keyboard focus should match :focus-visible`);
    assert(actual.outlineWidth>0&&actual.outlineStyle!=='none',`${label}: keyboard focus should have a visible outline`);
    assert(actual.visible,`${label}: focused control should be visible in the viewport`);
  };
  const assertLayout=async(label)=>{
    assert.equal(await ev('document.documentElement.scrollWidth>innerWidth'),false,`${label}: document should not overflow horizontally`);
    assert.equal(await ev("Array.from(document.querySelectorAll('.content')).some(x=>x.scrollWidth>x.clientWidth)"),false,`${label}: content should not overflow horizontally`);
  };
  const screenshot=async(name)=>{
    const shot=await send('Page.captureScreenshot',{format:'png'});
    fs.writeFileSync(path.join(os.tmpdir(),`vellora-audit-${name}.png`),Buffer.from(shot.data,'base64'));
  };
  await send('Page.enable');await send('Network.enable');
  await send('Fetch.enable',{patterns:[{urlPattern:'*',requestStage:'Request'}]});
  await send('Emulation.setFocusEmulationEnabled',{enabled:true});
  await send('Page.addScriptToEvaluateOnNewDocument',{source:mock});await send('Page.navigate',{url:'http://127.0.0.1:1420/'});await waitFor("!!document.querySelector('.nav')");
  assert(await ev("document.body.innerText.includes('Backend ready')"),'Initial backend health must be loaded');
  assert(await ev("document.body.innerText.includes('Dictation needs attention')"),'Recording error must be visible');
  assert(await ev("!!document.querySelector('.recovery-panel')"),'Failed paste must expose transcript recovery');
  await ev("window.__audit.emit('dictation-state-changed','idle')");await sleep(100);assert(await ev("document.body.innerText.includes('Dictation needs attention')"),'Error remains visible after overlay returns to idle');
  await click('Copy'); assert(await ev("document.body.innerText.includes('Copied to clipboard.')"));
  await ev("window.__audit.fail='copy_transcript'");await click('Copy'); assert(await ev("document.body.innerText.includes('Could not copy:')"));
  await ev("window.__audit.fail=null");await click('Paste in 3s');await click('Cancel paste (3s)');await sleep(3200);assert.equal(await ev("window.__audit.calls.filter(x=>x.cmd==='paste_transcript').length"),0,'Cancelled paste must never fire');
  await ev("document.querySelectorAll('.transcript-controls')[0].querySelectorAll('button')[1].click();document.querySelectorAll('.transcript-controls')[1].querySelectorAll('button')[1].click()");await sleep(100);assert.equal(await ev("Array.from(document.querySelectorAll('button')).filter(x=>x.textContent.includes('Cancel paste')).length"),1,'Only one paste countdown can be pending');await click('Cancel paste (3s)');
  await click('Paste in 3s');await nav('AI Models');await sleep(3200);assert.equal(await ev("window.__audit.calls.filter(x=>x.cmd==='paste_transcript').length"),0,'Leaving the page cancels its delayed paste');await nav('Dashboard');
  await click('Paste in 3s');await sleep(3300);assert.equal(await ev("window.__audit.calls.filter(x=>x.cmd==='paste_transcript').length"),1,'Scheduled paste fires once');
  await nav('AI Models');await ev("window.__audit.fail='save_settings'");await ev("Array.from(document.querySelectorAll('.model-card')).find(x=>x.textContent.includes('Whisper 1')).click()");await sleep(150);assert(await ev("document.body.innerText.includes('Could not save model:')"));assert.equal(await ev("window.__audit.settings.sttModel"),'gpt-4o-mini-transcribe','Model stays unchanged after save failure');
  await nav('Settings');await ev("document.getElementById('settings-tab-general').focus()");await key('ArrowRight');assert.equal(await ev('document.activeElement.id'),'settings-tab-appearance','Arrow keys navigate Settings tabs');await key('Home');assert.equal(await ev('document.activeElement.id'),'settings-tab-general');
  await ev("window.__audit.fail='save_openai_api_key';(()=>{const input=document.querySelector('input[type=password]');Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'audit-placeholder');input.dispatchEvent(new Event('input',{bubbles:true}));})()");await sleep(100);await ev("document.querySelector('form').dispatchEvent(new Event('submit',{bubbles:true,cancelable:true}))");await sleep(150);assert(await ev("document.body.innerText.includes('Could not save API key:')"));
  await ev("window.__audit.fail='delete_openai_api_key';document.querySelector('[aria-label=\"Remove saved API key\"]').click()");await sleep(100);assert(await ev("document.body.innerText.includes('Could not remove API key:')"));
  await ev("window.__audit.fail=null");
  for(const theme of ['classic','notebook']){
    await nav('Settings');await ev("document.getElementById('settings-tab-appearance').click()");await sleep(100);await ev(`document.querySelector('input[name=appearance][value=${theme}]').click()`);await sleep(150);
    for(const width of [1040,880,520]){
      await send('Emulation.setDeviceMetricsOverride',{width,height:620,deviceScaleFactor:1,mobile:false});
      for(const tab of ['Dashboard','History','AI Models','Settings','Help & About']){
        await nav(tab);
        assert.equal(await ev("document.documentElement.scrollWidth>innerWidth"),false,`${theme} ${tab} should not overflow at ${width}`);
        assert.equal(await ev("Array.from(document.querySelectorAll('.content')).some(x=>x.scrollWidth>x.clientWidth)"),false,`${theme} ${tab} content should not overflow at ${width}`);
        if(width===880&&['Dashboard','Settings','Help & About'].includes(tab)){const shot=await send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(os.tmpdir(),`vellora-audit-${theme}-${tab.toLowerCase().replace(/[^a-z]+/g,'-')}.png`),Buffer.from(shot.data,'base64'));}
      }
    }

    // Native Chromium Tab traversal and outlines, in both appearances. Establish
    // only the starting point programmatically; every transition uses CDP input.
    await send('Emulation.setDeviceMetricsOverride',{width:880,height:620,deviceScaleFactor:1,mobile:false});
    await nav('Dashboard');
    if(await ev("Array.from(document.querySelectorAll('button')).some(x=>x.textContent.trim()==='Dismiss')"))await click('Dismiss');
    await ev("document.querySelector('.nav button').focus()");
    await key('Tab');await assertFocus('.nav button:nth-child(2)',`${theme} Tab navigation`);
    await key('Tab',true);await assertFocus('.nav button:first-child',`${theme} Shift+Tab navigation`);
    await nav('Settings');await ev("document.getElementById('settings-tab-general').focus()");
    await key('Tab');await assertFocus('input[type=password]',`${theme} Tab enters settings fields`);
    await key('Tab',true);await assertFocus('#settings-tab-general',`${theme} Shift+Tab returns to settings tab`);
    await key('ArrowRight');await assertFocus('#settings-tab-appearance',`${theme} appearance tab focus`);
    await key('Home');await assertFocus('#settings-tab-general',`${theme} settings tab Home key`);

    // A long multiline note, an unbroken URL, Unicode and literal HTML should
    // remain text, wrap horizontally, and keep the bottom actions reachable.
    const originalHistory=await ev('structuredClone(window.__audit.history)');
    const originalSnapshot=await ev('structuredClone(window.__audit.snapshot)');
    const longText=('[DEMO] A long synthetic project note with punctuation, German umlauts äöü, and 日本語.\n').repeat(160)
      +'\nhttps://example.invalid/'+ 'x'.repeat(2048)
      +'\n<script>window.__audit.transcriptHtmlExecuted=true</script>\n[DEMO END]';
    await ev(`(()=>{const a=window.__audit;const text=${JSON.stringify(longText)};const item={...a.history[0],id:'audit-long',finalText:text,rawText:text,pasted:true,error:null,wordCount:text.split(/\\s+/).length,charCount:text.length};a.history=[item,...a.history];a.snapshot={state:'idle',message:null,lastTranscript:item};a.emit('history-changed');})()`);
    await nav('History');await waitFor("document.querySelector('.history-item > p')?.textContent===window.__audit.history[0].finalText");
    for(const width of [880,520]){
      await send('Emulation.setDeviceMetricsOverride',{width,height:620,deviceScaleFactor:1,mobile:false});
      await assertLayout(`${theme} long transcript ${width}px`);
      assert.equal(await ev("document.querySelector('.history-item > p').textContent.length"),longText.length,'Long transcript must not be truncated');
      assert.equal(await ev('!!window.__audit.transcriptHtmlExecuted'),false,'Transcript HTML must never execute');
      await ev("document.querySelector('.history-item .history-actions button').focus()");
      await key('Tab');await assertFocus('.history-item:first-child .history-actions button:nth-child(2)',`${theme} long-note Paste focus`);
      await key('Tab');await assertFocus('.history-item:first-child .history-actions button:nth-child(3)',`${theme} long-note Delete focus`);
    }
    await screenshot(`${theme}-long-transcript`);
    await ev(`window.__audit.history=${JSON.stringify(originalHistory)};window.__audit.snapshot=${JSON.stringify(originalSnapshot)};window.__audit.emit('history-changed')`);
    await waitFor("window.__audit.history.length===document.querySelectorAll('.history-item').length");

    await send('Emulation.setDeviceMetricsOverride',{width:880,height:620,deviceScaleFactor:1,mobile:false});
    await send('Emulation.setEmulatedMedia',{features:[{name:'forced-colors',value:'active'}]});
    assert(await ev("matchMedia('(forced-colors: active)').matches"),'Forced-colors emulation must be active');
    await nav('Dashboard');await ev("document.querySelector('.nav button').focus()");await key('Tab');
    await assertFocus('.nav button:nth-child(2)',`${theme} forced-colors keyboard focus`);
    const forcedContrast=await ev("(()=>{const style=getComputedStyle(document.activeElement);return {text:style.color,background:style.backgroundColor};})()");
    assert.notEqual(forcedContrast.text,forcedContrast.background,`${theme} forced-colors button text must remain distinguishable`);
    await assertLayout(`${theme} forced-colors`);await screenshot(`${theme}-forced-colors`);
    await send('Emulation.setEmulatedMedia',{features:[]});

    // The physical reference window is 880x620. Smaller CSS viewports plus DPR
    // emulate page reflow at 125%, 150% and 200%; native Windows scaling and true
    // browser toolbar zoom still need a separate human/native review.
    for(const scale of [1.25,1.5,2]){
      await send('Emulation.setDeviceMetricsOverride',{width:Math.round(880/scale),height:Math.round(620/scale),deviceScaleFactor:scale,mobile:false});
      assert(Math.abs(await ev('devicePixelRatio')-scale)<0.01,'DPI emulation must match requested scale');
      for(const tab of ['Dashboard','History','AI Models','Settings','Help & About']){
        await nav(tab);await assertLayout(`${theme} ${tab} ${Math.round(scale*100)}% reflow/DPI emulation`);
      }
      await nav('Dashboard');await ev("document.querySelector('.nav button').focus()");await key('Tab');
      await assertFocus('.nav button:nth-child(2)',`${theme} scaled keyboard navigation`);
      if(scale===2)await screenshot(`${theme}-200-percent-reflow`);
    }
    await send('Emulation.setDeviceMetricsOverride',{width:880,height:620,deviceScaleFactor:1,mobile:false});
    await nav('Help & About');await click('Copy support information');
    assert(await ev(`document.querySelector('.support-preview').value.includes(${JSON.stringify(appVersion)})`),'Support information must use the current package version');
    assert.equal(await ev("document.querySelector('.support-preview').value.includes('Private')"),false,'Support export excludes unexpected private fields');
    assert.equal(await ev("window.__audit.supportClipboard===document.querySelector('.support-preview').value"),true,'Support copy uses the isolated clipboard stub');
    await click('Open guided setup');
    for(let step=0;step<4;step++){
      for(const width of [1040,880,520]){
        await send('Emulation.setDeviceMetricsOverride',{width,height:620,deviceScaleFactor:1,mobile:false});
        assert.equal(await ev("document.documentElement.scrollWidth>innerWidth"),false,`${theme} setup ${step} should not overflow at ${width}`);
        assert.equal(await ev("Array.from(document.querySelectorAll('.content')).some(x=>x.scrollWidth>x.clientWidth)"),false,`${theme} setup ${step} content should not overflow at ${width}`);
        if(width===880&&step===1){const shot=await send('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(os.tmpdir(),`vellora-audit-${theme}-setup.png`),Buffer.from(shot.data,'base64'));}
      }
      if(step===1){await click('Check microphone');assert(await ev("document.body.innerText.includes('Synthetic microphone')"));}
      if(step<3)await click('Continue');
    }
    assert.equal(await ev("document.body.innerText.includes('A new transcript was created')"),false,'Old transcript must not count as setup practice');
    await click('Finish setup');
    assert.equal(await ev("localStorage.getItem('vellora.setup.v1')"),'complete');
  }
  await nav('Settings');await ev("document.getElementById('settings-tab-general').click()");await sleep(100);await ev("Array.from(document.querySelectorAll('.toggle-row')).find(x=>x.textContent.includes('Save transcript history')).querySelector('input').click()");await sleep(150);assert.equal(await ev('window.__audit.settings.saveHistory'),false);
  await nav('History');await click('Delete');assert.equal(await ev("window.__audit.calls.filter(x=>x.cmd==='delete_transcript').length"),0,'Delete asks for confirmation');assert.equal(await ev('document.activeElement.textContent'),'Cancel','Confirmation focuses safe cancel choice');await click('Cancel');assert.equal(await ev('document.activeElement.textContent.trim()'),'Delete','Cancelling restores trigger focus');assert.equal(await ev("window.__audit.history.length"),1);await click('Delete');await click('Delete transcript');await waitFor("window.__audit.history.length===0");
  await ev("window.__audit.history=[{id:'clear-example',createdAt:'2026-10-01T14:00:00Z',provider:'openai',sttModel:'whisper-1',cleanupMode:'off',rawText:'Fixture',finalText:'Fixture',wordCount:1,charCount:7,pasted:true}];window.__audit.emit('history-changed')");await sleep(150);await click('Clear all history');assert.equal(await ev("window.__audit.calls.filter(x=>x.cmd==='clear_transcripts').length"),0);await click('Delete all transcripts');await waitFor("window.__audit.history.length===0");
  assert.deepEqual(await ev('window.__audit.unhandled'),[],'No unhandled promise failures');
  assert.equal(await ev('window.__audit.networkAttempts.length'),0,'Main-window fixture must not attempt external networking');

  // Render the actual overlay component in this synthetic tab. This checks
  // reduced-motion CSS on its real animated waveform/spinner, not a fake probe.
  await send('Page.addScriptToEvaluateOnNewDocument',{source:"window.__TAURI_INTERNALS__.metadata={currentWindow:{label:'overlay'},currentWebview:{label:'overlay',windowLabel:'overlay'}}"});
  await send('Page.reload');await waitFor("!!document.querySelector('.overlay-root')");
  await waitFor("window.__audit.calls.some(x=>x.cmd==='plugin:event|listen'&&x.args.event==='overlay-state-changed')");
  for(const theme of ['classic','notebook']){
    await ev(`window.__audit.settings.appearance=${JSON.stringify(theme)};window.__audit.emit('settings-changed',window.__audit.settings);window.__audit.emit('overlay-state-changed',{state:'transcribing',message:'Synthetic processing',level:0.5})`);
    await waitFor("!!document.querySelector('.waveform.animated span')");
    await send('Emulation.setEmulatedMedia',{features:[{name:'prefers-reduced-motion',value:'reduce'}]});
    assert(await ev("matchMedia('(prefers-reduced-motion: reduce)').matches"),'Reduced-motion emulation must be active');
    const motions=await ev("Array.from(document.querySelectorAll('.waveform span,.spin,.overlay-pill')).map(el=>{const s=getComputedStyle(el);return {animation:s.animationName,transition:s.transitionDuration};})");
    assert(motions.length>=3,'Actual overlay animation elements must be rendered');
    assert(motions.every(x=>x.animation==='none'&&x.transition.split(',').every(v=>parseFloat(v)===0)),`${theme} reduced motion must disable waveform/spinner animation and transitions`);
    assert.equal(await ev("document.getAnimations().filter(x=>x.playState==='running').length"),0,`${theme} overlay must not retain running animations`);
    await screenshot(`${theme}-reduced-motion-overlay`);
    await send('Emulation.setEmulatedMedia',{features:[]});
  }
  assert.deepEqual(await ev('window.__audit.unhandled'),[],'No unhandled overlay promise failures');
  assert.equal(await ev('window.__audit.networkAttempts.length'),0,'Overlay fixture must not attempt external networking');
  assert.equal(externalRequests.length,0,'Synthetic frontend tests must not make external/provider requests');
  assert.equal(interceptionErrors.length,0,'Loopback request interception must remain active and successful');
  console.log(`PASS: Vellora ${appVersion}; failure feedback, truthful persistence, recovery, paste cancellation, history confirmations, real Tab/Shift+Tab/focus-visible, long text, forced colors, reduced-motion overlay, 125/150/200% viewport/DPI emulation, guided setup/support privacy, both themes/all pages, no unhandled promises.`);
  await send('Page.close');
  ws.close();
})().catch(e=>{console.error(`GUI smoke failed: ${e.message}`);process.exit(1)});
