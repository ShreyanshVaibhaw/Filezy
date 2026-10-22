// Cases for ExtraCursor's evaluate tool. Prepare a disposable fixture with
// --prepare; --json <fixture> returns named expressions for the tool to run.
// This file never launches apps or sends real mouse/keyboard input.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function cases(root) {
  const folder = JSON.stringify(root);
  const child = JSON.stringify(path.join(root, "child"));
  const wrap = (body) => `(async()=>{const e=__e2e; const check=(v,m)=>{if(!v)throw new Error(m)}; const sleep=ms=>new Promise(r=>setTimeout(r,ms)); const settle=async()=>{for(let i=0;i<200;i++){if(e.idle()){await sleep(60);return}await sleep(25)}throw new Error('Listing did not settle')};${body};return {passed:true}})()`;
  return [
    { name: "explicit folder launch survives ExtraCursor's debugger switch", js: wrap(`check(e.app.path.toLowerCase()===${folder}.toLowerCase(),'Launch did not open the requested folder')`) },
    { name: "Content metadata and virtualization in a large folder", js: wrap(`await e.navigate(${folder});e.setView('content');await settle();check(e.app.total>=3000,'Large fixture missing');check(document.querySelectorAll('.contentrow').length<100,'Content rendered the entire folder');check(document.querySelector('.contentmeta')?.innerText.includes('Date modified'),'Content metadata missing');document.querySelector('.scroller').scrollTop=100000;await sleep(200);check(document.querySelectorAll('.contentrow').length<100,'Scrolling lost virtualization');document.querySelector('.scroller').scrollTop=0`) },
    { name: "Content preferences and selection survive navigation and refresh", js: wrap(`await e.navigate(${folder});e.setView('content');await e.selectNames(['example.txt']);await e.refresh();check((await e.selectedNames()).includes('example.txt'),'Refresh lost selection');await e.navigate(${child});e.setView('tiles');await e.navigate(${folder});check(e.app.view==='content','Folder view was not restored')`) },
    { name: "Details columns scroll with their headers at minimum width", js: wrap(`await e.win.setSize(new e.LogicalSize(500,320));e.setView('details');await sleep(150);const sc=document.querySelector('.scroller');check(sc.scrollWidth>sc.clientWidth,'Clipped columns cannot scroll');sc.scrollLeft=sc.scrollWidth;await sleep(100);check(document.querySelector('.header').getBoundingClientRect().left<0,'Header did not follow horizontal scrolling');const header=[...document.querySelectorAll('.hcell')].find(b=>b.innerText.includes('Size'));check(header.getBoundingClientRect().right<=innerWidth+2,'Size column still unreachable');sc.scrollLeft=0`) },
    { name: "View and file menus fit at the minimum window size", js: wrap(`const view=[...document.querySelectorAll('.cbtn')].find(b=>b.innerText.includes('View'));view.click();await sleep(150);let menu=document.querySelector('.menu');let r=menu.getBoundingClientRect();check(r.left>=0&&r.top>=0&&r.right<=innerWidth&&r.bottom<=innerHeight,'View menu exceeds viewport');menu.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await sleep(50);const row=document.querySelector('[data-idx]');row.dispatchEvent(new MouseEvent('contextmenu',{clientX:innerWidth-1,clientY:innerHeight-1,bubbles:true,cancelable:true}));await sleep(400);menu=document.querySelector('.menu');r=menu.getBoundingClientRect();check(r.right<=innerWidth&&r.bottom<=innerHeight,'File menu exceeds viewport');menu.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))`) },
    { name: "Expanded translations remain usable at minimum width", js: wrap(`e.setMessages({'View':'A translated view command with a much longer label','Settings':'Translated application preferences and settings'},true);await sleep(100);check(document.documentElement.scrollWidth<=innerWidth,'Expanded text widened the window');const bar=document.querySelector('.commandbar');check(bar.scrollWidth>bar.clientWidth,'Long commands are not reachable by scrolling');const cap=document.querySelector('.caption').getBoundingClientRect();check(cap.right<=innerWidth,'Window controls moved offscreen');e.setMessages({},false)`) },
    { name: "Pane dimensions are bounded and persisted", js: wrap(`await e.win.setSize(new e.LogicalSize(1280,820));e.setSetting('navWidth',320);e.setSetting('paneWidth',350);e.setSetting('detailsPane',true);await sleep(150);check(Math.round(document.querySelector('.navpane').getBoundingClientRect().width)===320,'Navigation width wrong');check(Math.round(document.querySelector('.sidepane').getBoundingClientRect().width)===350,'Details width wrong');check(JSON.parse(localStorage.getItem('settings')).navWidth===320,'Width not stored');e.setSetting('detailsPane',false)`) },
    { name: "Many tabs keep the active tab and caption controls visible", js: wrap(`for(let i=0;i<24;i++)await e.newTab(e.HOME);await sleep(100);const active=document.querySelector('[role=tab][aria-selected=true]').getBoundingClientRect();const strip=document.querySelector('.tabs').getBoundingClientRect();check(active.left>=strip.left-1&&active.right<=strip.right+1,'Active tab is hidden');check(document.querySelector('.caption').getBoundingClientRect().right<=innerWidth,'Caption clipped');check(document.querySelector('.tabpicker'),'All tabs picker missing');while(e.tabs.length>1)e.closeTab(e.tabs[e.tabs.length-1].id);await e.navigate(${folder})`) },
    { name: "Tab records preserve history and reject damaged input", js: wrap(`await e.navigate(${folder});await e.navigate(${child});const record=await e.exportTab(e.app.activeTab);check(record.hist.includes(${folder})&&record.path===${child},'Export lost history');let refused=false;try{await e.importTab({path:'bad',hist:[]})}catch{refused=true}check(refused,'Malformed tab was accepted');await e.importTab(record);check(e.tabs.at(-1).hist.length===record.hist.length,'Import lost history');e.closeTab(e.app.activeTab);await e.navigate(${folder})`) },
    { name: "Device adapter lists shell identities and handles unavailable locations", js: wrap(`const folder=await e.invoke('shell_list',{path:${folder},devicesOnly:false});check(folder.items.some(i=>i.name==='example.txt'&&i.path.endsWith('example.txt')&&i.copy),'Shell item identity or capability missing');await e.navigate('::Devices');for(let i=0;i<100&&e.shellFolder.loading;i++)await sleep(50);check(!e.can('new')&&!e.can('paste')&&!e.can('change'),'Device writes enabled');check(document.querySelector('.shellpane')?.innerText.includes('unavailable'),'Missing capability explanation');await e.navigate('::Shell:'+encodeURIComponent('::{00000000-0000-0000-0000-000000000000}'));for(let i=0;i<100&&e.shellFolder.loading;i++)await sleep(50);check(e.shellFolder.error.length>0,'Unavailable device has no error');await e.navigate(${folder})`) },
    { name: "Single-click opening respects modified selection and the default", js: wrap(`await e.navigate(${folder});e.setView('content');e.setSetting('singleClick',false);await settle();let row=[...document.querySelectorAll('[data-idx]')].find(n=>n.innerText.startsWith('child'));row.click();check(e.app.path===${folder},'Default single click opened a folder');e.setSetting('singleClick',true);row.dispatchEvent(new MouseEvent('click',{button:0,ctrlKey:true,bubbles:true}));check(e.app.path===${folder},'Ctrl click opened a folder');row.click();await sleep(150);check(e.app.path===${child},'Single click did not open folder');e.setSetting('singleClick',false);await e.navigate(${folder})`) },
    { name: "Light and dark palettes retain readable opaque backgrounds", js: wrap(`for(const theme of ['win11-light','win11-dark']){e.setTheme(theme);await sleep(100);const root=getComputedStyle(document.querySelector('.root'));check(!root.backgroundColor.endsWith(', 0)')&&root.backgroundColor!=='rgba(0, 0, 0, 0)','Transparent app background');check(root.color!==root.backgroundColor,'Text matches background')}e.setTheme('system')`) },
    { name: "Renderer layouts at 100, 125, 150 and 200 percent", js: wrap(`await e.win.setSize(new e.LogicalSize(1000,700));const os=await e.win.scaleFactor();try{for(const dpr of [1,1.25,1.5,2]){await e.invoke('e2e_zoom',{scale:dpr/os});await sleep(200);check(Math.abs(devicePixelRatio-dpr)<0.02,'Renderer scale did not change');check(document.documentElement.scrollWidth<=innerWidth,'Scaled layout exceeds viewport');const cap=document.querySelector('.caption').getBoundingClientRect();check(cap.right<=innerWidth+1&&cap.left>=0,'Scaled caption controls are clipped');check(document.querySelector('.scroller').clientHeight>80,'Scaled list is unusable')}}finally{await e.invoke('e2e_zoom',{scale:1})}`) },
    { name: "Native window transitions across attached monitors", js: wrap(`await e.win.setSize(new e.LogicalSize(900,600));const displays=await e.invoke('e2e_displays',{index:null});const seen=[];try{for(let i=0;i<displays.length;i++){await e.invoke('e2e_displays',{index:i});await sleep(350);const scale=await e.win.scaleFactor();const at=await e.win.outerPosition();check(at.x>=displays[i].x&&at.x<displays[i].x+displays[i].width,'Window did not move to monitor');check(Math.abs(devicePixelRatio-scale)<0.02,'Renderer did not follow native monitor DPI');check(document.querySelector('.caption').getBoundingClientRect().right<=innerWidth+1,'Caption clipped after monitor transition');seen.push({index:i,nativeScale:scale,rendererScale:devicePixelRatio})}}finally{await e.invoke('e2e_displays',{index:0})}return {passed:true,displays,seen}`) },
    { name: "Device selection and scroll survive tab switching and refresh", js: wrap(`await e.navigate('::Shell:'+encodeURIComponent(${folder}));await sleep(100);check(e.shellFolder.items.length>=3000,'Device fixture missing');check(document.querySelectorAll('.item').length<100,'Device list lost virtualization');document.querySelectorAll('.item input')[1].click();const selected=[...e.shellSelection.paths];const id=e.app.activeTab;document.querySelector('.shellpane').scrollTop=1800;await sleep(100);await e.newTab(e.HOME);const temporary=e.app.activeTab;await e.switchTab(id);await sleep(100);check(document.querySelector('.shellpane').scrollTop===1800,'Switching tabs lost device scroll');await e.refresh();await sleep(100);check(document.querySelector('.shellpane').scrollTop===1800,'Refresh lost device scroll');check(JSON.stringify(e.shellSelection.paths)===JSON.stringify(selected),'Device selection was lost');e.closeTab(temporary);await e.navigate(${folder})`) },
  ];
}

export function performanceCase(root) {
  return { name: "warm 100,000-file listing, search and scrolling", js: `(async()=>{
    const e=__e2e;const folder=${JSON.stringify(root)};
    const median=values=>[...values].sort((a,b)=>a-b)[Math.floor(values.length/2)];
    const listings=[];const search=[];e.setView('details');e.setSetting('previewPane',false);e.setSetting('detailsPane',false);
    for(let i=0;i<5;i++){await e.navigate(e.HOME);const start=performance.now();await e.navigate(folder);await new Promise(requestAnimationFrame);listings.push(performance.now()-start)}
    if(e.app.total!==100000)throw new Error('Expected the 100,000-file fixture');
    const frames=[];const scroller=document.querySelector('.scroller');let previous=performance.now();const deadline=previous+4500;
    while(frames.length<120&&performance.now()<deadline){await Promise.race([new Promise(requestAnimationFrame),new Promise(r=>setTimeout(r,150))]);const now=performance.now();frames.push(now-previous);previous=now;scroller.scrollTop+=scroller.clientHeight}
    scroller.scrollTop=0;
    for(let i=0;i<3;i++){const start=performance.now();await e.startSearch('item-099999',true);while(e.app.searching&&performance.now()-start<5000)await new Promise(r=>setTimeout(r,20));search.push(performance.now()-start);await e.clearSearch()}
    const sorted=[...frames].sort((a,b)=>a-b);
    return globalThis.__filezyPerf={build:'debug, e2e feature',cache:'warm OS cache; files just created; other applications running',files:100000,
      listingMs:listings,listingMedianMs:median(listings),searchMs:search,searchMedianMs:median(search),
      frameCount:frames.length,frameMedianMs:median(frames),frameP95Ms:sorted[Math.floor(sorted.length*.95)],
      over34ms:frames.filter(ms=>ms>34).length,viewport:{width:innerWidth,height:innerHeight,dpr:devicePixelRatio},
      rendererTiming:performance.getEntriesByType('navigation')[0]?.toJSON()};
  })()` };
}

// This uses the production UI, without enabling test hooks in a release build.
export function productionPerformanceCase(root) {
  return { name: "production UI warm 100,000-file performance", js: `(async()=>{
    const folder=${JSON.stringify(root)};const sleep=ms=>new Promise(r=>setTimeout(r,ms));
    const until=async(test)=>{const deadline=performance.now()+10000;while(!test()){if(performance.now()>deadline)throw Error('UI did not settle');await sleep(10)}};
    const go=async(path)=>{document.querySelector('.addressbox').click();await sleep(0);const input=document.querySelector('.addressbox input');input.value=path;input.dispatchEvent(new Event('input',{bubbles:true}));input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));await until(()=>path==='::Home'?document.querySelector('.homepane'):document.querySelector('.scroller')?.dataset.drop===path&&document.querySelector('[aria-setsize="100000"]'));await new Promise(requestAnimationFrame)};
    const median=values=>[...values].sort((a,b)=>a-b)[Math.floor(values.length/2)];
    const listings=[];for(let i=0;i<5;i++){await go('::Home');const start=performance.now();await go(folder);listings.push(performance.now()-start)}
    const sc=document.querySelector('.scroller');const frames=[];let previous=performance.now();const deadline=previous+4500;
    while(frames.length<120&&performance.now()<deadline){await Promise.race([new Promise(requestAnimationFrame),sleep(150)]);const now=performance.now();frames.push(now-previous);previous=now;sc.scrollTop+=sc.clientHeight}sc.scrollTop=0;
    const search=[];for(let i=0;i<3;i++){const input=document.querySelector('input[aria-label="Search"]');input.value='item-099999';input.dispatchEvent(new Event('input',{bubbles:true}));const start=performance.now();input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));await until(()=>document.querySelector('.statusbar .stat')?.innerText==='1 item'&&!document.querySelector('.stopbtn'));search.push(performance.now()-start);input.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await until(()=>document.querySelector('[aria-setsize="100000"]'))}
    const sorted=[...frames].sort((a,b)=>a-b);return globalThis.__filezyPerf={build:'production release, no test feature',cache:'warm OS cache; other applications running',files:100000,listingMs:listings,listingMedianMs:median(listings),searchMs:search,searchMedianMs:median(search),frameCount:frames.length,frameMedianMs:median(frames),frameP95Ms:sorted[Math.floor(sorted.length*.95)],over34ms:frames.filter(ms=>ms>34).length,viewport:{width:innerWidth,height:innerHeight,dpr:devicePixelRatio},view:document.querySelector('.contentrow')?'content':'details',rendererTiming:performance.getEntriesByType('navigation')[0]?.toJSON()};
  })()` };
}

export function wslCaseRegression(root) {
  if (!/^\\\\wsl\.localhost\\Ubuntu\\tmp\\filezy-native-[\w-]+$/.test(root)) throw new Error("Use the disposable Ubuntu fixture only.");
  return { name: "WSL case-distinct copies and rename collision protection", js: `(async()=>{
    const e=__e2e;const root=${JSON.stringify(root)};const dest=await e.invoke('create_folder',{dir:root});
    const result=await e.invoke('copy_into',{srcs:[root+'\\\\Case.txt',root+'\\\\case.txt'],dest});
    if(result.items.length!==2||result.items.some(i=>i.state!=='done'))throw new Error('Case-distinct outcomes were merged or failed');
    e.recordExternalCopy(result);if(e.history.undo.at(-1).items.length!==2)throw new Error('Undo record lost a case-distinct item');
    await e.navigate(dest);const rows=await e.listing();const index=rows.findIndex(i=>i.name==='Case.txt');
    await e.commitRename(index,'case.txt');const names=(await e.listing()).map(i=>i.name);
    const text=await e.invoke('preview_text',{path:dest+'\\\\case.txt'});
    if(!names.includes('Case.txt')||!names.includes('case.txt')||text.text!=='lower'||!e.app.status.includes('already'))throw new Error('Rename overwrote a different Linux file');
    return {passed:true,dest,reported:result.items.length,status:e.app.status};
  })()` };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  if (process.argv[2] === "--prepare") {
    fs.mkdirSync("scratchpad", { recursive: true });
    const root = fs.mkdtempSync(path.resolve("scratchpad", "extracursor-"));
    fs.mkdirSync(path.join(root, "child"));
    fs.writeFileSync(path.join(root, "example.txt"), "Filezy native test fixture.\n");
    fs.writeFileSync(path.join(root, "child", "日本語.txt"), "Unicode fixture.\n");
    for (let i = 0; i < 3000; i++) fs.writeFileSync(path.join(root, `item-${String(i).padStart(5, "0")}.txt`), "");
    console.log(root);
  } else if (process.argv[2] === "--json" && process.argv[3]) {
    const root=path.resolve(process.argv[3]);
    console.log(JSON.stringify(process.argv.includes('--wsl') ? [wslCaseRegression(process.argv[3])] : process.argv.includes('--ui-perf') ? [productionPerformanceCase(root)] : process.argv.includes('--perf') ? [performanceCase(root)] : cases(root)));
  }
}
