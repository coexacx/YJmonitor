import {agentVersionLabel,compareVersions} from "/assets/version-status-005a4f2e5674.js";
import {createPager} from "/assets/pagination-d84897e63d5d.js";
export const taskLabel=t=>({pending:'等待升级',running:'执行中',done:'已完成',failed:'失败',retry:'等待重试'}[t?.state]||'—');
export function createAgentMaintenance({el,api,toast,button,getRecords,refresh}){
 const pager=createPager(el,()=>update(),50,'Agent 版本列表');
 let root=null,rows=null,summary=null,selectAll=null,run=null,retry=null,selected=new Set(),submitting=false,result=null,version='',release=null;
 function eligible(r){return r.canUpgrade===true&&compareVersions(version,r.agentVersion)===1;}
 function checkbox(label,checked,change){
  const c=el('input');c.type='checkbox';c.checked=checked;c.setAttribute('aria-label',label);c.addEventListener('change',()=>change(c.checked));return c;
 }
 function render(panel,target){
  version=target;root=el('section','agent-maintenance');const toolbar=el('div','agent-maintenance-toolbar');
  const allLabel=el('label','check-field');selectAll=checkbox('全选可升级的在线服务器',false,checked=>{selected=new Set(checked?getRecords().filter(eligible).map(r=>r.public.id):[]);update();});
  allLabel.append(selectAll,document.createTextNode('全选可升级的在线服务器'));
  run=button('升级所选','primary-button',()=>submit([...selected]));run.dataset.batchUpgrade='';
  retry=button('重试失败项','secondary-button',()=>submit(getRecords().filter(r=>eligible(r)&&r.management?.action==='upgrade'&&r.management?.state==='failed').map(r=>r.public.id)));retry.dataset.batchRetry='';
  summary=el('p','form-footnote');summary.setAttribute('role','status');result=el('p','form-footnote batch-submit-result');result.setAttribute('role','status');rows=el('div','table-scroll');
  toolbar.append(allLabel,run,retry);root.append(toolbar,el('p','form-footnote','配套版本 '+version+' · 最多同时升级 2 台。任务由主控执行，关闭页面后仍会继续。'),summary,result,rows,pager.root);panel.append(root);update();
 }
 function update(){
  if(!root?.isConnected)return;
  const records=getRecords(),available=records.filter(eligible),availableIds=new Set(available.map(r=>r.public.id));
  for(const id of selected)if(!availableIds.has(id))selected.delete(id);
  const tasks=records.map(r=>r.management).filter(t=>t?.action==='upgrade'),count=state=>tasks.filter(t=>state.includes(t.state)).length;
  summary.textContent='已选 '+selected.size+' 台 · 等待 '+count(['pending'])+' · 执行中 '+count(['running'])+' · 完成 '+count(['done'])+' · 失败 '+count(['failed','retry']);
  run.textContent='升级所选'+(selected.size?'（'+selected.size+'）':'');run.disabled=submitting||!selected.size;
  retry.disabled=submitting||!available.some(r=>r.management?.action==='upgrade'&&r.management?.state==='failed');
  selectAll.checked=available.length>0&&selected.size===available.length;selectAll.indeterminate=selected.size>0&&selected.size<available.length;selectAll.disabled=submitting||!available.length;
  const active=document.activeElement,focusNode=active?.dataset.upgradeNode;
  const table=el('table','node-table agent-task-table'),thead=el('thead'),header=el('tr');
  for(const s of ['选择','服务器 / Agent','升级任务','操作'])header.append(el('th','',s));thead.append(header);table.append(thead);const tbody=el('tbody');
  const range=pager.range(records.length);
  for(const r of records.slice(range.start,range.end)){
   const tr=el('tr');tr.dataset.id=r.public.id;const pick=el('td'),c=checkbox('选择 '+r.public.name,selected.has(r.public.id),checked=>{if(checked)selected.add(r.public.id);else selected.delete(r.public.id);update();});
   c.dataset.upgradeNode=r.public.id;c.disabled=submitting||!eligible(r);c.title=r.upgradeBlocked||'';pick.append(c);
   const name=el('td');name.append(el('strong','',r.public.name),el('small','agent-version',(r.public.online?'在线':'离线')+' · '+(r.agentVersion||'待上报')),el('small','agent-version-status',agentVersionLabel(r,release,version)));
   const state=el('td'),t=r.management;
   state.append(el('strong','task-state '+(t?.state||''),taskLabel(t)),el('small','deploy-status',t?.message||r.upgradeBlocked||'可以升级'));
   if(t?.updatedAt)state.append(el('small','agent-version',new Date(t.updatedAt*1000).toLocaleString('zh-CN',{hour12:false})));
   const actions=el('td','agent-row-actions'),b=button(t?.state==='failed'?'重试':'升级 Agent','secondary-button compact-button',()=>submit([r.public.id]));b.disabled=submitting||!eligible(r);b.title=r.upgradeBlocked||'';
   if(!r.removing){
    actions.append(b);
    const rollback=button('回退 Agent','secondary-button compact-button',()=>single('agent-rollback',r.public.id));rollback.disabled=submitting||['pending','running','retry'].includes(t?.state);actions.append(rollback);
    if(t?.state==='retry'){const retryTask=button('重试任务','secondary-button compact-button',()=>single('migration/retry',r.public.id));retryTask.disabled=submitting;actions.append(retryTask);}
   }
   tr.append(pick,name,state,actions);tbody.append(tr);
  }
  table.append(tbody);rows.replaceChildren(table);if(!records.length)rows.append(el('p','admin-empty','还没有服务器。'));
  if(focusNode)[...rows.querySelectorAll('[data-upgrade-node]')].find(c=>c.dataset.upgradeNode===focusNode)?.focus({preventScroll:true});
 }
 async function single(action,id){
  if(submitting)return;submitting=true;update();
  try{await api('/api/admin/ops/'+action,'POST',{id});await refresh(true);toast('任务已提交');}catch(e){toast(e.message,true);}
  finally{submitting=false;update();}
 }
 async function submit(ids){
  if(submitting||!ids.length)return;submitting=true;update();
  try{
   const r=await api('/api/admin/ops/agents/batch-update','POST',{ids});
   const names=new Map(getRecords().map(n=>[n.public.id,n.public.name]));
   const message='已提交 '+r.accepted.length+' 台'+(r.rejected.length?'；未提交：'+r.rejected.map(v=>(names.get(v.id)||v.id)+'（'+v.message+'）').join('、'):'。');
   if(result)result.textContent=message;for(const id of r.accepted)selected.delete(id);toast(message,!r.accepted.length);
   await refresh(true);
  }catch(e){if(result)result.textContent=e.message;toast(e.message,true);}
  finally{submitting=false;update();}
 }
 function clear(){selected.clear();pager.reset();root=null;result=null;release=null;}
 function setRelease(value){release=value;update();}
 return {render,update,clear,setRelease};
}
