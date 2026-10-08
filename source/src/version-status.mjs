export function compareVersions(a,b){
 const parse=v=>/^\d{1,5}\.\d{1,5}\.\d{1,5}$/.test(v||'')?v.split('.').map(Number):null;
 const x=parse(a),y=parse(b);if(!x||!y)return null;
 for(let i=0;i<3;i++)if(x[i]!==y[i])return x[i]>y[i]?1:-1;return 0;
}
export function agentVersionLabel(record,release,target){
 const installed=record.agentVersion,compatible=compareVersions(installed,target);
 if(compatible===null)return '等待版本上报';
 if(release?.agentLatest&&!release.stale){
  const latest=compareVersions(installed,release.agentLatest);
  if(latest===0)return '已是最新版本';
  if(latest>0)return '当前版本高于发布版';
  if(compareVersions(release.agentLatest,target)>0)return '发现新版 '+release.agentLatest+' · 请先更新主控';
 }
 if(compatible<0)return '可升级至 '+target;
 if(compatible>0)return '当前版本高于配套版';
 return '已是配套版本 · '+(release?.error?'检测暂不可用':'等待新版检测');
}
export function updateNotice(release,records){
 if(!release||release.stale)return '';
 const parts=[];
 if(release.available)parts.push('主控 '+release.latest);
 const count=records.filter(r=>compareVersions(release.agentLatest,r.agentVersion)===1).length;
 if(count)parts.push('Agent '+release.agentLatest+'（'+count+' 台）');
 return parts.length?'发现新版 · '+parts.join(' · '):'';
}
