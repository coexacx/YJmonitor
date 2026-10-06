import {EditorState, Compartment} from '@codemirror/state';
import {EditorView, lineNumbers, highlightActiveLineGutter, keymap, highlightActiveLine, drawSelection} from '@codemirror/view';
import {defaultKeymap, history, historyKeymap, indentWithTab} from '@codemirror/commands';
import {search, searchKeymap, openSearchPanel} from '@codemirror/search';
import {syntaxHighlighting, HighlightStyle, StreamLanguage, bracketMatching} from '@codemirror/language';
import {javascript} from '@codemirror/lang-javascript';
import {json} from '@codemirror/lang-json';
import {css} from '@codemirror/lang-css';
import {html} from '@codemirror/lang-html';
import {python} from '@codemirror/lang-python';
import {yaml} from '@codemirror/lang-yaml';
import {shell} from '@codemirror/legacy-modes/mode/shell';
import {diffLines} from 'diff';
import {tags} from '@lezer/highlight';
// The editor retains the original dark PHP palette independently of the white
// workspace chrome. Light-theme syntax colors are unreadable on this surface.
const editorHighlight=HighlightStyle.define([
 {tag:tags.comment,color:'#9bb0bd'},
 {tag:[tags.keyword,tags.modifier,tags.operatorKeyword],color:'#cbb9ed'},
 {tag:[tags.string,tags.regexp],color:'#b5d6c5'},
 {tag:[tags.number,tags.bool,tags.null],color:'#e4c193'},
 {tag:[tags.typeName,tags.className,tags.tagName],color:'#8dcfd0'},
 {tag:[tags.propertyName,tags.attributeName],color:'#a9cce6'},
 {tag:[tags.meta,tags.annotation],color:'#b5c6d9'},
 {tag:tags.invalid,color:'#f0aaa6',textDecoration:'underline'}
]);
const lang=path=>{const ext=path.split('.').pop().toLowerCase();if(['js','mjs','cjs','ts','tsx','jsx'].includes(ext))return javascript({typescript:ext.startsWith('t'),jsx:ext.endsWith('x')});if(ext==='json')return json();if(ext==='css')return css();if(['html','htm','xml','svg'].includes(ext))return html();if(ext==='py')return python();if(['yaml','yml'].includes(ext))return yaml();if(['sh','bash','zsh'].includes(ext)||/\/\.(bashrc|profile)$/.test(path))return StreamLanguage.define(shell);return [];};
export function createEditor(input,save){
 const mount=document.createElement('div');mount.className='file-code-editor';input.after(mount);input.hidden=true;
 const readonly=new Compartment(),language=new Compartment(),wrap=new Compartment();let updating=false;
 const view=new EditorView({parent:mount,state:EditorState.create({doc:input.value,extensions:[
 EditorState.phrases.of({'Find':'查找','Replace':'替换','next':'下一个','previous':'上一个','all':'全选','match case':'区分大小写','by word':'完整单词','regexp':'正则表达式','replace':'替换','replace all':'全部替换','close':'关闭','current match':'当前匹配','replaced $ matches':'已替换 $ 处'}),
 lineNumbers(),highlightActiveLineGutter(),history(),drawSelection(),highlightActiveLine(),bracketMatching(),
 syntaxHighlighting(editorHighlight),search({top:true}),
 keymap.of([{key:'Mod-s',run:()=>{save();return true;}},...defaultKeymap,...historyKeymap,...searchKeymap,indentWithTab]),
 readonly.of(EditorState.readOnly.of(false)),language.of([]),wrap.of([]),
 EditorView.contentAttributes.of({'aria-label':'文件内容',spellcheck:'false',autocapitalize:'off',autocorrect:'off'}),
 EditorView.updateListener.of(u=>{if(u.docChanged&&!updating){input.value=u.state.doc.toString();input.dispatchEvent(new Event('input'));}}),
 EditorView.theme({
  '&':{height:'100%',color:'#d7e3eb',backgroundColor:'#111c25'},
  '.cm-scroller':{overflow:'auto',fontFamily:'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',fontSize:'13px',lineHeight:'1.6'},
  '.cm-content':{padding:'12px 0',caretColor:'#c8e6dc'},
  '.cm-gutters':{backgroundColor:'#111c25',color:'#9bb0bd',borderRight:'1px solid #263b46'},
  '.cm-activeLine,.cm-activeLineGutter':{backgroundColor:'#1b2b36'},
  '.cm-panels':{backgroundColor:'#192a33',color:'#d7e3eb'},
  '.cm-search input':{color:'#d7e3eb',backgroundColor:'#111c25',maxWidth:'160px'},
  '.cm-selectionBackground':{backgroundColor:'#406584 !important'},
  '.cm-cursor':{borderLeftColor:'#c8e6dc'},
  '.cm-searchMatch':{backgroundColor:'#50613880',outline:'1px solid #8fae65'},
  '.cm-searchMatch-selected':{backgroundColor:'#40658480',outline:'1px solid #8fb6d4'},
  '&.cm-focused .cm-matchingBracket':{backgroundColor:'#40658480',color:'#fff'}
 },{dark:true})
 ]})});
 function sync(path='',reset=false){updating=true;const text=input.value;const changes=view.state.doc.toString()===text?undefined:{from:0,to:view.state.doc.length,insert:text};view.dispatch({changes,effects:[readonly.reconfigure(EditorState.readOnly.of(input.readOnly||input.disabled)),...(path?[language.reconfigure(lang(path))]:[])]});updating=false;if(reset)view.dispatch({selection:{anchor:0}});}
 return {sync,wrap:value=>view.dispatch({effects:wrap.reconfigure(value?EditorView.lineWrapping:[])}),search:()=>openSearchPanel(view),focus:()=>view.focus(),destroy:()=>{view.destroy();mount.remove();}};
}
export function reviewChanges(scope,before,after,path){
 return new Promise(resolve=>{
 const d=document.createElement('dialog');d.className='node-dialog file-diff-dialog';
 const title=document.createElement('h2');title.textContent='保存前核对';
 const name=document.createElement('p');name.textContent=path;
 const body=document.createElement('div');body.className='file-diff-content';
 const parts=diffLines(before,after,{timeout:150,maxEditLength:4000});
 if(!parts){const p=document.createElement('p');p.textContent='修改范围较大，请返回编辑器核对全文后保存。';body.append(p);}
 else {let shown=0;for(const part of parts){if(!part.added&&!part.removed){const lines=part.value.split('\n');const p=document.createElement('pre');p.textContent=lines.length>8?lines.slice(0,3).join('\n')+'\n… '+(lines.length-6)+' 行未修改 …\n'+lines.slice(-3).join('\n'):part.value;body.append(p);continue;}const p=document.createElement('pre');p.className=part.added?'diff-added':'diff-removed';const text=part.value.slice(0,65536-shown);p.textContent=(part.added?'+ ':'− ')+text;shown+=text.length;body.append(p);if(shown>=65536){const note=document.createElement('p');note.textContent='预览已截断，请在编辑器核对完整内容。';body.append(note);break;}}}
 const actions=document.createElement('div');actions.className='dialog-actions';const cancel=document.createElement('button'),save=document.createElement('button');cancel.type=save.type='button';cancel.textContent='继续编辑';save.textContent='确认保存';save.className='primary-button';
 const finish=v=>{d.close();d.remove();resolve(v);};cancel.onclick=()=>finish(false);save.onclick=()=>finish(true);d.oncancel=e=>{e.preventDefault();finish(false);};d.append(title,name,body,actions);actions.append(cancel,save);scope.append(d);d.showModal();cancel.focus();
 });
}

