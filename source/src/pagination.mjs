export function pageWindow(total,page,size){
 const pages=Math.max(1,Math.ceil(total/size)),current=Math.max(1,Math.min(pages,Math.trunc(Number(page)||1)));
 return {page:current,pages,start:(current-1)*size,end:Math.min(current*size,total)};
}
export function createPager(el,onChange,size,label){
 let page=1,total=0;
 const root=el('nav','list-pagination');root.setAttribute('aria-label',label);
 const prev=el('button','secondary-button','上一页'),next=el('button','secondary-button','下一页'),select=el('select'),count=el('span');
 prev.type=next.type='button';select.setAttribute('aria-label',label+'页码');
 root.append(prev,select,count,next);
 function go(value){page=pageWindow(total,value,size).page;onChange();}
 prev.onclick=()=>go(page-1);next.onclick=()=>go(page+1);select.onchange=()=>go(select.value);
 function range(length){
  total=length;const w=pageWindow(total,page,size);page=w.page;root.hidden=total<=size;
  if(select.options.length!==w.pages)select.replaceChildren(...Array.from({length:w.pages},(_,i)=>new Option('第 '+(i+1)+' 页',String(i+1))));
  select.value=String(page);count.textContent='共 '+w.pages+' 页 · '+total+' 台';
  prev.disabled=page<=1;next.disabled=page>=w.pages;return w;
 }
 return {root,range,reset:()=>{page=1;}};
}
