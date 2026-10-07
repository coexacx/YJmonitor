import test from 'node:test';
import assert from 'node:assert/strict';
import {createFileChannel} from '../src/terminal-file-channel.mjs';
test('file connection has its own authorization, buffer limit and lifetime',async()=>{
 const originals={WebSocket:globalThis.WebSocket,location:globalThis.location};
 const sockets=[];
 class Socket {
  static OPEN=1;
  constructor(url){this.url=url;this.readyState=1;this.bufferedAmount=0;this.sent=[];sockets.push(this);}
  send(value){this.sent.push(JSON.parse(value));}
  close(){this.readyState=3;}
 }
 globalThis.WebSocket=Socket;globalThis.location={href:'https://panel.example/'};
 let messages=0,readies=0,disconnected=0;
 const channel=createFileChannel({api:async(path,method,body)=>{assert.equal(body.id,'node');return {ticket:'one-use'};},onReady:()=>readies++,onMessage:()=>messages++,onDisconnect:()=>disconnected++,onStatus:()=>{}});
 try{
  channel.open({node:'node',session:'lease'});await new Promise(setImmediate);
  const first=sockets[0];assert.equal(first.url,'wss://panel.example/api/terminal');
  first.onopen();assert.deepEqual(first.sent[0],{type:'authorize',ticket:'one-use',mode:'files',transferSession:'lease'});
  assert.throws(()=>channel.send('{}'));
  first.onmessage({data:JSON.stringify({type:'ready',files:true})});assert.equal(readies,1);
  first.bufferedAmount=512*1024+1;assert.throws(()=>channel.send('{}'));assert.equal(first.readyState,1);
  first.bufferedAmount=0;channel.send(JSON.stringify({type:'file',id:'x'}));
  first.onmessage({data:JSON.stringify({type:'file_result',id:'x'})});assert.equal(messages,1);
  channel.stop();assert.equal(first.readyState,3);assert.equal(disconnected,0);
  channel.open({node:'node',session:'lease'});await new Promise(setImmediate);sockets[1].onopen();
  assert.equal(sockets[1].sent.length,1);assert.equal(sockets[1].sent[0].type,'authorize');
  channel.stop();assert.throws(()=>channel.send('{}'));
 }finally{channel.stop();Object.assign(globalThis,originals);}
});
test('a late authorization response cannot reopen a closed file connection',async()=>{
 const old=globalThis.WebSocket;let called=false,resolve;
 globalThis.WebSocket=class{constructor(){called=true;}};
 const channel=createFileChannel({api:()=>new Promise(r=>resolve=r),onReady:()=>{},onMessage:()=>{},onDisconnect:()=>{},onStatus:()=>{}});
 channel.open({node:'node',session:'lease'});channel.stop();resolve({ticket:'late'});
 await new Promise(setImmediate);assert.equal(called,false);globalThis.WebSocket=old;
});
