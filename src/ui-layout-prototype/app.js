// PROTOTYPE ONLY: simulated windows; no real processes, persistence, or mutations.
const paths = {
 logo:'<rect x="3" y="4" width="15" height="12" rx="2"/><path d="M7 8h7M7 12h4"/><rect x="12" y="13" width="9" height="7" rx="1.5"/>',
 window:'<rect x="3" y="4" width="18" height="16" rx="3"/><path d="M3 9h18M7 6.5h.01M10 6.5h.01"/>',
 search:'<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 4 4"/>',
 refresh:'<path d="M20 7v5h-5M4 17v-5h5"/><path d="M6.1 7a7 7 0 0 1 11.6-2L20 8M4 16l2.3 3A7 7 0 0 0 17.9 17"/>',
 shrink:'<path d="M4 4h16v16H4zM4 9h16M16 12v4h-4m4 0-5-5"/>',
 maximize:'<path d="M4 9V4h5m6 0h5v5m0 6v5h-5M9 20H4v-5m10-5 6-6M4 20l6-6"/>',
 hide:'<path d="m3 3 18 18M10.6 10.6a2 2 0 0 0 2.8 2.8M8.5 5.7A11 11 0 0 1 12 5c7 0 10 7 10 7a17 17 0 0 1-3.3 4.2M6 6.6A17 17 0 0 0 2 12s3 7 10 7a11 11 0 0 0 5.5-1.4"/>',
 show:'<path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Z"/><circle cx="12" cy="12" r="3"/>',
 browser:'<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a16 16 0 0 1 0 18 16 16 0 0 1 0-18Z"/>',
 document:'<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9ZM14 3v6h6M8 13h8M8 17h5"/>',
 editor:'<path d="m8 7-5 5 5 5m8-10 5 5-5 5m-3-12-2 14"/>',
 music:'<path d="M9 18V5l11-2v13M9 9l11-2"/><ellipse cx="6" cy="18" rx="3" ry="3"/><ellipse cx="17" cy="16" rx="3" ry="3"/>',
 close:'<path d="m6 6 12 12M6 18 18 6"/>',minimize:'<path d="M5 13h14"/>',
 tray:'<path d="M4 14h4l2 3h4l2-3h4v6H4zM12 3v9m-4-4 4 4 4-4"/>',check:'<path d="m5 12 4 4L19 6"/>',arrow:'<path d="M5 12h14m-5-5 5 5-5 5"/>'
};
const icon=(name,size=18)=>`<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name]||paths.window}</svg>`;
const esc=text=>String(text).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const variants={A:{name:'紧凑列表',description:'A · 把更多窗口放在同一屏，选好后在底部统一操作。'},B:{name:'双栏操作台',description:'B · 左侧选择窗口，右侧持续展示当前选择与批量操作。'},C:{name:'按程序分组',description:'C · 把同一程序的窗口放在一起，先找到一组，再统一处理。'}};
const groups={game:'游戏窗口',browser:'浏览器',document:'文档',editor:'编辑器',music:'音乐'};
const initialWindows=[
 {id:1,title:'游戏窗口 01',program:'GameClient.exe',pid:23880,kind:'game',width:1280,height:720,mode:'normal'},
 {id:2,title:'游戏窗口 02',program:'GameClient.exe',pid:24612,kind:'game',width:240,height:135,mode:'normal',affected:true,result:'已缩小至 240 × 135'},
 {id:3,title:'游戏窗口 03',program:'GameClient.exe',pid:25136,kind:'game',width:1280,height:720,mode:'normal',hidden:true,result:'已隐藏'},
 {id:4,title:'游戏窗口 04',program:'GameClient.exe',pid:26724,kind:'game',width:1280,height:720,mode:'normal'},
 {id:5,title:'资料查阅',program:'Browser.exe',pid:19440,kind:'browser',width:1600,height:900,mode:'normal'},
 {id:6,title:'使用说明.pdf',program:'PDFViewer.exe',pid:30216,kind:'document',width:900,height:680,mode:'normal'},
 {id:7,title:'项目工作区',program:'Code.exe',pid:18712,kind:'editor',width:1440,height:900,mode:'normal',unavailable:true,result:'需要相同权限'},
 {id:8,title:'音乐播放器',program:'Music.exe',pid:19524,kind:'music',width:960,height:540,mode:'minimized'}
];
const requestedVariant=new URL(location.href).searchParams.get('variant');
const state={variant:variants[requestedVariant]?requestedVariant:'A'};
let actionTimer,toastTimer;
function resetData(){clearTimeout(actionTimer);state.windows=initialWindows.map((w,i)=>{const result={...w,x:80+i*24,y:80+i*18,hidden:!!w.hidden,affected:!!w.affected,failed:false,result:w.result||'',baseline:{width:w.kind==='game'?1280:w.width,height:w.kind==='game'?720:w.height}};if(result.hidden)result.beforeHide={width:result.width,height:result.height,x:result.x,y:result.y,mode:result.mode};return result;});Object.assign(state,{width:'240',query:'',selected:new Set([1,2]),busy:false,failure:false,view:'main',exitErrors:[],message:'已找到 8 个桌面窗口',error:false});}
resetData();
function filteredWindows(){const q=state.query.trim().toLowerCase();return state.windows.filter(w=>`${w.title} ${w.program} ${w.pid}`.toLowerCase().includes(q));}
function selectedWindows(){return state.windows.filter(w=>state.selected.has(w.id));}
function filteredSelected(){const shown=new Set(filteredWindows().map(w=>w.id));return [...state.selected].filter(id=>!shown.has(id)).length;}
function validWidth(){return /^\d+$/.test(state.width.trim())&&Number(state.width)>0&&Number(state.width)<=2147483647;}
function disabled(extra=false){return state.busy||extra?'disabled':'';}
function button(action,label,iconName,style='',extra=false){return `<button class="btn ${style}" data-action="${action}" ${disabled(extra)}>${icon(iconName)}<span>${label}</span></button>`;}
function search(){return `<div class="search">${icon('search')}<input data-field="query" type="search" value="${esc(state.query)}" placeholder="搜索标题、程序或 PID" aria-label="搜索窗口" ${disabled()}><button data-action="clear-search" class="search-clear ${state.query?'':'invisible'}" aria-label="清空搜索" ${disabled()}>×</button></div>`;}
function countSummary(full=true){const outside=filteredSelected();return `<div class="select-summary"><span>已选 <strong>${state.selected.size}</strong> 个${full?'窗口':''}</span>${outside?`<span class="filter-note">${outside} 个不在当前列表</span>`:''}</div>`;}
function clearSelection(){return `<button class="inline-link" data-action="clear" ${disabled(!state.selected.size)}>清空勾选</button>`;}
function widthControl(){return `<div class="width-control"><label for="target-width">目标宽度</label><div class="width-input-wrap ${validWidth()?'':'invalid'}"><input id="target-width" data-field="width" value="${esc(state.width)}" inputmode="numeric" aria-label="目标内容区宽度" ${disabled()}><span>px</span></div></div>`;}
function actionGroups(){const empty=!state.selected.size;return `<div class="action-groups"><div class="action-group">${button('shrink','缩小','shrink','primary',empty||!validWidth())}${button('maximize','放大','maximize','',empty)}</div><span class="divider"></span><div class="action-group">${button('hide','隐藏','hide','',empty)}${button('show','显示','show','soft',empty)}</div></div>`;}
function caption(){return `<div class="caption"><span class="caption-brand">${icon('logo',14)}窗口缩放器</span><div class="caption-controls"><button class="caption-button" data-action="tray" aria-label="收起主界面到托盘" title="收起主界面到托盘">${icon('minimize')}</button><button class="caption-button" data-action="preview-size" aria-label="切换预览尺寸" title="切换预览尺寸">${icon('window')}</button><button class="caption-button close" data-action="tray" aria-label="关闭主界面并收起到托盘" title="关闭主界面并收起到托盘">${icon('close')}</button></div></div>`;}
function heading(title='窗口管理',subtitle='选择窗口，统一调整大小与可见性'){return `<div class="app-heading"><span class="brand-mark">${icon('logo',22)}</span><div><h2>${title}</h2><p>${subtitle}</p></div></div>`;}
function statusBar(){return `<footer class="app-status"><span class="status-line ${state.error?'failure-line':''}"><i class="status-dot"></i>${esc(state.message)}</span><button class="exit-button" data-action="exit" ${disabled()} title="显示本次隐藏的窗口，并放大本次缩小的窗口后退出">正常退出 ${icon('arrow',11)}</button></footer>`;}
function badge(w){let label='可见',style='visible';if(w.unavailable){label='权限不足';style='error';}else if(w.hidden){label='已隐藏';style='hidden';}else if(w.mode==='minimized'){label='最小化';style='';}else if(w.mode==='maximized'){label='最大化';style='visible';}else if(w.affected){label='已缩小';style='small';}return `<span class="badge ${style}">${label}</span>`;}
function rowSize(w){return w.mode==='minimized'?'—':`${w.width} × ${w.height}`;}
function appIcon(w){return `<span class="app-icon ${w.kind}">${icon(w.kind==='game'?'window':w.kind)}</span>`;}
function rowCheck(w){return `<input type="checkbox" data-check="${w.id}" aria-label="选择 ${esc(w.title)}" ${state.selected.has(w.id)?'checked':''} ${disabled(w.unavailable)}>`;}
function allCheck(){const available=filteredWindows().filter(w=>!w.unavailable),all=available.length>0&&available.every(w=>state.selected.has(w.id));return `<input type="checkbox" data-all aria-label="全选当前列表" title="仅选择当前列表中的可操作窗口" ${all?'checked':''} ${disabled(!available.length)}>`;}
function resultLabel(w){return `<span class="row-result ${w.failed?'failed':w.result?'success':''}" title="${esc(w.result||'尚未操作')}">${esc(w.result||'—')}</span>`;}
function empty(){return `<div class="empty">${icon('search',29)}<strong>没有匹配的窗口</strong>换个标题、程序名或 PID 试试</div>`;}
function rowA(w){return `<div class="data-row ${state.selected.has(w.id)?'selected':''} ${w.unavailable?'unavailable':''}" data-row="${w.id}"><span class="check-wrap">${rowCheck(w)}</span><div class="window-name">${appIcon(w)}<div class="name-text"><strong>${esc(w.title)}</strong><div class="subline">${esc(w.program)}<span class="dot">·</span>PID ${w.pid}</div></div></div>${badge(w)}<span class="row-size">${rowSize(w)}</span>${resultLabel(w)}</div>`;}
function VariantA(){const rows=filteredWindows();return `<div class="app-layout layout-a"><header class="app-header">${heading()}<span class="small-count">${state.windows.length} 个窗口</span></header><div class="a-body"><div class="toolbar a-toolbar">${search()}${button('refresh','刷新','refresh','')}<span class="spacer"></span>${clearSelection()}</div><div class="table-meta">${countSummary()}<span>当前列表 ${rows.length} / ${state.windows.length}</span></div><div class="window-table a-table"><div class="column-head"><span class="check-wrap">${allCheck()}</span><span class="head-title">窗口 / 程序</span><span>状态</span><span>内容区尺寸</span><span>操作结果</span></div><div class="table-scroll">${rows.length?rows.map(rowA).join(''):empty()}</div></div></div><div class="a-dock"><div class="a-dock-label">操作 ${state.selected.size} 个已选窗口<small>${filteredSelected()?`包括列表外的 ${filteredSelected()} 个窗口`:'更改大小 · 管理可见性'}</small></div><div class="a-dock-controls">${widthControl()}${actionGroups()}</div></div>${statusBar()}</div>`;}
function rowB(w){return `<div class="data-row b-row ${state.selected.has(w.id)?'selected':''} ${w.unavailable?'unavailable':''}" data-row="${w.id}"><span class="check-wrap">${rowCheck(w)}</span><div class="window-name">${appIcon(w)}<div class="name-text"><strong>${esc(w.title)}</strong><div class="subline">PID ${w.pid}<span class="dot">·</span>${rowSize(w)}<span class="dot">·</span>${esc(w.program)}</div></div></div><div class="b-row-end">${badge(w)}${resultLabel(w)}</div></div>`;}
function VariantB(){const rows=filteredWindows(),selected=selectedWindows();return `<div class="app-layout layout-b"><header class="b-topbar">${heading('窗口操作台','列表选择，右侧集中操作')}<span class="header-note">${state.windows.length} 个桌面窗口</span></header><div class="b-workspace"><section class="b-list-pane"><div class="toolbar">${search()}${button('refresh','刷新','refresh','')}</div><div class="table-meta"><span>当前列表 ${rows.length} 个</span>${clearSelection()}</div><div class="window-table b-table"><div class="column-head">${allCheck()}<span>窗口 / 进程 / 尺寸</span><span class="spacer"></span><span>状态</span></div><div class="table-scroll">${rows.length?rows.map(rowB).join(''):empty()}</div></div></section><aside class="b-inspector"><div><div class="inspector-kicker">当前选择</div><div class="inspector-count"><b>${selected.length}</b><span>个窗口</span></div><div class="selected-chips">${selected.length?selected.slice(0,2).map(w=>`<span class="selection-chip">${esc(w.title)}</span>`).join('')+(selected.length>2?`<span class="selection-chip">+${selected.length-2}</span>`:''):'<span class="empty-chip">从左侧列表勾选窗口</span>'}</div></div>${filteredSelected()?`<div class="hidden-selection-banner">${filteredSelected()} 个已选窗口不在当前列表，仍参与本次操作。</div>`:''}<section class="inspector-section"><h3>窗口大小</h3>${widthControl()}<div class="b-actions-grid">${button('shrink','缩小','shrink','primary',!selected.length||!validWidth())}${button('maximize','放大','maximize','',!selected.length)}</div></section><section class="inspector-section"><h3>窗口可见性</h3><div class="b-actions-grid">${button('hide','隐藏','hide','',!selected.length)}${button('show','显示','show','soft',!selected.length)}</div></section><p class="b-inspector-note">显示会保留隐藏前的位置和大小。<br>正常退出时自动显示隐藏的窗口。</p><div class="inspector-bottom"><span>${state.busy?'正在处理…':'对全部已选窗口生效'}</span></div></aside></div>${statusBar()}</div>`;}
function card(w){return `<div class="window-card ${state.selected.has(w.id)?'selected':''} ${w.unavailable?'unavailable':''}" data-row="${w.id}"><div class="card-top">${rowCheck(w)}${appIcon(w)}<div class="name-text"><strong>${esc(w.title)}</strong><div class="subline">PID ${w.pid}</div></div></div><div class="card-bottom">${badge(w)}${resultLabel(w)}<span class="row-size">${rowSize(w)}</span></div></div>`;}
function VariantC(){const rows=filteredWindows(),kinds=[...new Set(rows.map(w=>w.kind))];return `<div class="app-layout layout-c"><header class="c-header">${heading('窗口工作台','按程序找到窗口，按选择统一操作')}${search()}${button('refresh','刷新','refresh','')}</header><div class="c-commandbar">${countSummary(false)}${clearSelection()}<span class="divider"></span>${widthControl()}${actionGroups()}</div><div class="c-body">${kinds.length?kinds.map(kind=>{const list=rows.filter(w=>w.kind===kind);return `<section class="program-group"><div class="group-heading"><strong>${groups[kind]}</strong><span>${esc(list[0].program)} · ${list.length} 个</span><button class="inline-link" data-action="group" data-group="${kind}" ${disabled(list.every(w=>w.unavailable))}>${list.filter(w=>!w.unavailable).every(w=>state.selected.has(w.id))?'取消本组':'选择本组'}</button></div><div class="group-grid">${list.map(card).join('')}</div></section>`;}).join(''):empty()}</div>${statusBar()}</div>`;}
function suspended(){const tray=state.view==='tray';return `<div class="suspended"><div class="suspended-inner"><span class="suspended-icon">${icon(tray?'tray':'check',30)}</span><h2>${tray?'主界面已收起到托盘':'工具已退出'}</h2><p>${tray?'可从托盘重新打开主界面，或正常退出工具。':esc(state.message)}</p><div class="suspended-actions">${button('open',tray?'显示主界面':'重新打开','window','primary')}${tray?button('exit','正常退出','arrow',''):''}</div><p class="simulation-note">原型中的托盘与退出演示</p></div></div>`;}
function exitDialog(){if(!state.exitErrors.length)return '';return `<div class="modal-scrim"><section class="exit-dialog" role="alertdialog" aria-modal="true" aria-labelledby="exit-title"><span class="dialog-label">退出前恢复窗口</span><h3 id="exit-title">${state.exitErrors.length} 个窗口尚未完成恢复</h3><p>显示或放大失败。重试后确认结果，或选择仍然退出。</p><div class="dialog-errors">${state.exitErrors.map(w=>`<div>${esc(w.title)}<small>目标暂未接受操作</small></div>`).join('')}</div><div class="dialog-actions">${button('force-exit','仍然退出','arrow','')}${button('retry-exit','重试并退出','refresh','primary')}</div></section></div>`;}
function render(){
 const focused=document.activeElement,field=focused?.dataset.field,selection=field?[focused.selectionStart,focused.selectionEnd]:null;
 const scroll={};document.querySelectorAll('.table-scroll,.c-body,.b-inspector').forEach(el=>scroll[el.className]=el.scrollTop);
 document.getElementById('app').innerHTML=caption()+(state.view==='main'?({A:VariantA,B:VariantB,C:VariantC}[state.variant])():suspended())+exitDialog();
 document.querySelectorAll('.table-scroll,.c-body,.b-inspector').forEach(el=>{el.scrollTop=scroll[el.className]||0;});
 if(field){const replacement=document.querySelector(`[data-field="${field}"]`);if(replacement&&!replacement.disabled){replacement.focus({preventScroll:true});if(selection[0]!==null)replacement.setSelectionRange(...selection);}}
 const all=document.querySelector('[data-all]');if(all){const available=filteredWindows().filter(w=>!w.unavailable),chosen=available.filter(w=>state.selected.has(w.id));all.indeterminate=chosen.length>0&&chosen.length<available.length;}
 document.getElementById('variant-description').textContent=variants[state.variant].description;
 document.getElementById('variant-tabs').innerHTML=Object.entries(variants).map(([key,value])=>`<button role="tab" class="variant-tab" data-variant="${key}" aria-selected="${state.variant===key}" aria-label="方案 ${key} ${value.name}"><b>${key}</b>${value.name}</button>`).join('');
 document.getElementById('failure-toggle').setAttribute('aria-pressed',String(state.failure));document.getElementById('failure-toggle').textContent=state.failure?'部分失败：已开启':'模拟部分失败';
 document.getElementById('state-dump').textContent=JSON.stringify({布局:state.variant,目标宽度:state.width,搜索:state.query,已选:[...state.selected],列表外已选:filteredSelected(),工具状态:state.view,反馈:state.message,窗口:state.windows.map(w=>({标题:w.title,PID:w.pid,已选:state.selected.has(w.id),隐藏:w.hidden,状态:w.mode,尺寸:[w.width,w.height],位置:[w.x,w.y],结果:w.result}))},null,2);
}
function toast(message){clearTimeout(toastTimer);const el=document.getElementById('toast');el.textContent=message;el.classList.add('show');toastTimer=setTimeout(()=>el.classList.remove('show'),2500);}
function switchVariant(variant){state.variant=variant;const url=new URL(location.href);url.searchParams.set('variant',variant);history.replaceState(null,'',url);render();}
function cycle(direction){const keys=Object.keys(variants);switchVariant(keys[(keys.indexOf(state.variant)+direction+keys.length)%keys.length]);}
function toggleSize(){const narrow=document.getElementById('desktop').classList.toggle('narrow');const control=document.getElementById('size-toggle');control.setAttribute('aria-pressed',String(narrow));control.textContent=narrow?'返回标准尺寸':'紧凑尺寸';}
function selectRow(id){if(state.busy||state.exitErrors.length)return;const w=state.windows.find(w=>w.id===id);if(!w||w.unavailable)return;state.selected.has(id)?state.selected.delete(id):state.selected.add(id);render();}
function showWindow(w){if(w.hidden){Object.assign(w,w.beforeHide||{});w.hidden=false;delete w.beforeHide;}}
function applyAction(w,action){
 if(action==='hide'){if(!w.hidden)w.beforeHide={width:w.width,height:w.height,x:w.x,y:w.y,mode:w.mode};w.hidden=true;w.result='已隐藏';}
 if(action==='show'){showWindow(w);w.result='已显示';}
 if(action==='shrink'){const width=Number(state.width),height=Math.round(width*w.baseline.height/w.baseline.width);if(width>1920||height>1032){w.result='请求尺寸超出工作区';w.failed=true;return false;}showWindow(w);Object.assign(w,{width,height,mode:'normal',affected:true,result:`已缩小至 ${width} × ${height}`});}
 if(action==='maximize'){showWindow(w);Object.assign(w,{width:1600,height:900,mode:'maximized',result:'已最大化'});}
 w.failed=false;return true;
}
function perform(action){if(state.busy||!state.selected.size||state.exitErrors.length)return;if(action==='shrink'&&!validWidth()){toast('请输入有效的正整数宽度');return;}const targets=selectedWindows(),labels={shrink:'缩小',maximize:'放大',hide:'隐藏',show:'显示'};state.busy=true;state.error=false;state.message=`正在${labels[action]} ${targets.length} 个已选窗口…`;render();actionTimer=setTimeout(()=>{let failed=0;for(const [index,w]of targets.entries()){if(state.failure&&index===targets.length-1){w.result='目标暂未接受操作';w.failed=true;failed++;}else if(!applyAction(w,action)){failed++;}}state.busy=false;state.error=failed>0;state.message=`${targets.length-failed} 个${labels[action]}成功${failed?`，${failed} 个未完成`:''}`;render();toast(state.message);},450);}
function exit(){if(state.busy)return;const targets=state.windows.filter(w=>w.hidden||w.affected);let shown=0,large=0;state.exitErrors=[];for(const [index,w]of targets.entries()){if(state.failure&&index===targets.length-1){state.exitErrors.push(w);continue;}if(w.hidden){showWindow(w);shown++;}if(w.affected){applyAction(w,'maximize');large++;}}if(state.exitErrors.length){state.error=true;state.view='main';state.message=`${state.exitErrors.length} 个窗口尚未完成退出恢复`;render();return;}state.message=`已显示 ${shown} 个隐藏窗口，放大 ${large} 个缩小过的窗口。`;state.error=false;state.view='exited';render();}
function handle(action,element){
 if(action==='preview-size'){toggleSize();return;}
 if(action==='tray'){if(state.exitErrors.length)return;state.view='tray';render();return;}
 if(action==='open'){state.view='main';if(!state.exitErrors.length)state.message=`已找到 ${state.windows.length} 个桌面窗口`;render();return;}
 if(action==='exit'){exit();return;}
 if(action==='retry-exit'){state.failure=false;state.exitErrors=[];exit();return;}
 if(action==='force-exit'){state.message=`已退出，${state.exitErrors.length} 个目标窗口仍未恢复。`;state.view='exited';state.exitErrors=[];render();return;}
 if(state.busy||state.exitErrors.length)return;
 if(action==='clear-search'){state.query='';render();return;}
 if(action==='clear'){state.selected.clear();render();return;}
 if(action==='refresh'){state.message=`列表已刷新，共 ${state.windows.length} 个窗口`;state.error=false;render();toast(state.message);return;}
 if(action==='group'){const group=filteredWindows().filter(w=>w.kind===element.dataset.group&&!w.unavailable),clear=group.every(w=>state.selected.has(w.id));for(const w of group)clear?state.selected.delete(w.id):state.selected.add(w.id);render();return;}
 perform(action);
}
document.getElementById('app').addEventListener('click',event=>{const control=event.target.closest('[data-action]');if(control&&!control.disabled){handle(control.dataset.action,control);return;}const row=event.target.closest('[data-row]');if(row&&event.target.tagName!=='INPUT')selectRow(Number(row.dataset.row));});
document.getElementById('app').addEventListener('change',event=>{if(event.target.matches('[data-check]'))selectRow(Number(event.target.dataset.check));if(event.target.matches('[data-all]')){for(const w of filteredWindows().filter(w=>!w.unavailable))event.target.checked?state.selected.add(w.id):state.selected.delete(w.id);render();}});
document.getElementById('app').addEventListener('input',event=>{const field=event.target.dataset.field;if(field){state[field]=event.target.value;render();}});
document.getElementById('variant-tabs').addEventListener('click',event=>{const tab=event.target.closest('[data-variant]');if(tab)switchVariant(tab.dataset.variant);});
document.getElementById('previous').addEventListener('click',()=>cycle(-1));document.getElementById('next').addEventListener('click',()=>cycle(1));
document.getElementById('reset').addEventListener('click',()=>{resetData();render();toast('演示已重置');});
document.getElementById('size-toggle').addEventListener('click',toggleSize);document.getElementById('failure-toggle').addEventListener('click',()=>{state.failure=!state.failure;render();});
document.addEventListener('keydown',event=>{if(event.target.matches('input,textarea,select')||event.target.isContentEditable||event.altKey||event.ctrlKey||event.metaKey)return;if(event.key==='ArrowLeft'||event.key==='ArrowRight'){event.preventDefault();cycle(event.key==='ArrowRight'?1:-1);}});
render();
