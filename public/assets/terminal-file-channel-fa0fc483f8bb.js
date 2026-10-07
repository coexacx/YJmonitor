import {TerminalRetry,canRetryRequest,canRetryClose} from "/assets/terminal-retry-9f33960c93a8.js";
// Each attached terminal owns one independent, bounded browser file transport.
export function createFileChannel({api,onReady,onMessage,onDisconnect,onStatus}){
 let target=null,socket=null,epoch=0,ready=false;
 const retry=new TerminalRetry({retry:connect,waiting:delay=>onStatus('文件连接中断，'+delay/1000+' 秒后重试；终端仍可使用。')});
 function dispose(){ready=false;epoch++;if(socket){socket.onclose=null;socket.close(1000,'closed');socket=null;}}
 function stop(){target=null;retry.reset();dispose();}
 async function connect(){
  if(!target)return;dispose();const run=epoch,context=target;
  try{
   const ticket=await api('/api/admin/terminal-ticket','POST',{id:context.node});
   if(run!==epoch||context!==target)return;
   const ws=new WebSocket(new URL('/api/terminal',location.href).href.replace(/^https:/,'wss:'));socket=ws;let decision=null;
   ws.onopen=()=>{if(run===epoch)ws.send(JSON.stringify({type:'authorize',ticket:ticket.ticket,mode:'files',transferSession:context.session}));};
   ws.onmessage=event=>{
    if(run!==epoch)return;
    let message;try{message=JSON.parse(event.data);}catch{decision=false;ws.close(1000,'invalid response');return;}
    if(message.type==='ready'&&message.files===true){ready=true;retry.connected();onReady(context);}
    else if(message.type==='file_result')onMessage(message);
    else if(message.type==='error'){decision=message.retryable===true;onStatus(message.message||'文件连接失败');}
   };
   ws.onclose=event=>{if(run!==epoch)return;ready=false;socket=null;onDisconnect();if(target&&(canRetryClose(event.code,decision)||decision===null&&event.code===1005)){if(!retry.schedule())onStatus('文件自动重试已结束，请重新打开终端后继续。');}};
   ws.onerror=()=>{};
  }catch(e){if(run!==epoch)return;onDisconnect();onStatus(e.message);if(target&&canRetryRequest(e)&&!retry.schedule())onStatus('文件自动重试已结束，请重新打开终端后继续。');}
 }
 return {open(context){stop();target=context;connect();},stop,send(data){if(!ready||socket?.readyState!==WebSocket.OPEN)throw Error('文件通道正在连接，请稍后重试');if(socket.bufferedAmount>512*1024)throw Error('文件发送队列繁忙，请稍后重试');socket.send(data);}};
}
