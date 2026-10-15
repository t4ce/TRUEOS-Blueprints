// Controlled synthetic MCP over verified TLS, HTTP/2 DATA frames or HTTP/1.1.
// Keys under tls/ are public test-fixture material, never account credentials.
import fs from 'node:fs';
import http2 from 'node:http2';
import {fileURLToPath} from 'node:url';

const tlsDir = fileURLToPath(new URL('./tls/', import.meta.url));
const port = Number(process.argv[2] ?? 18768);
const wrongPort = Number(process.argv[3] ?? 18769);
const tools = Array.from({length:106}, (_,i)=>({name:`synthetic_tool_${String(i).padStart(3,'0')}`, description:'synthetic description '+'x'.repeat(3000),inputSchema:{type:'object',properties:{}}}));
const cases = new Set(['json','sse','404','401','reset','headers_timeout','body_timeout','cancel','recovery']);
function log(fields) {
    console.log(JSON.stringify({...fields,monotonic_ns:process.hrtime.bigint().toString(),unix_ms:Date.now()}));
}
function serve(prefix, listenPort, wrongRoot) {
    const server = http2.createSecureServer({key:fs.readFileSync(`${tlsDir}/${prefix}key.pem`),cert:fs.readFileSync(`${tlsDir}/${prefix}cert.pem`),allowHTTP1:true});
    server.on('tlsClientError', error=>{
        // This listener sees only our synthetic fixture handshakes. Emit the
        // OpenSSL static code/reason, never arbitrary messages or peer data.
        const code = /^[A-Z0-9_]{1,100}$/.test(error.code ?? '') ? error.code : 'OTHER';
        const reason = /^[a-z0-9 _-]{1,120}$/i.test(error.reason ?? '') ? error.reason : 'other';
        log({stage:'tls_rejected',wrong_root:Number(wrongRoot),unknown_ca:Number(error.code==='ERR_SSL_TLSV1_ALERT_UNKNOWN_CA'||error.message.includes('unknown ca')),tls_code:code,tls_reason:reason});
    });
    server.on('sessionError', ()=>log({stage:'session_error',wrong_root:Number(wrongRoot)}));
    server.on('request',(req,res)=>{
        const name = req.url.replace(/^\//,'');
        const protocol = req.httpVersionMajor === 2 ? 2 : 1;
        const alpn_h2 = Number(req.socket.alpnProtocol === 'h2');
        const reply = (status, data='')=>{
            res.writeHead(status,{'content-type':'application/json','content-length':Buffer.byteLength(data)});
            res.end(data);
        };
        req.on('error',()=>{});res.on('error',()=>{});
        if(wrongRoot){log({stage:'unexpected_wrong_root_request',protocol,alpn_h2});reply(500);return;}
        if(!cases.has(name)){reply(404);return;}
        if(req.method==='GET'){reply(405);return;}
        if(req.method==='DELETE'){reply(204);return;}
        if(req.method!=='POST'){reply(405);return;}
        const chunks=[];
        req.on('data',chunk=>chunks.push(chunk));
        req.on('end',async()=>{
            let message;
            try{message=JSON.parse(Buffer.concat(chunks));}catch{reply(400);return;}
            const stage = message.method==='initialize'?'initialize':message.method==='notifications/initialized'?'initialized':message.method==='tools/list'?'tools_list':'other';
            log({case:name,stage,protocol,alpn_h2});
            if(stage==='initialize'){
                reply(200,JSON.stringify({jsonrpc:'2.0',id:message.id,result:{protocolVersion:'2025-06-18',capabilities:{tools:{}},serverInfo:{name:'synthetic-fixture',version:'1'}}}));return;
            }
            if(stage==='initialized'){reply(202);return;}
            if(stage!=='tools_list'){reply(400);return;}
            if(name==='401'||name==='404'){reply(Number(name),'{}');return;}
            if(name==='reset'){res.destroy();return;}
            const holdOpen=()=>{
                const start=Date.now();
                let reported=false;
                const watchdog=setTimeout(()=>{reported=true;log({case:name,stage:'stream_end',peer_closed:0,duration_ms:Date.now()-start,protocol,alpn_h2});res.destroy();},8000);
                res.once('close',()=>{if(!reported){reported=true;clearTimeout(watchdog);log({case:name,stage:'stream_end',peer_closed:1,duration_ms:Date.now()-start,protocol,alpn_h2});}});
            };
            if(name==='headers_timeout'||name==='cancel'){holdOpen();return;}
            const data=JSON.stringify({jsonrpc:'2.0',id:message.id,result:{tools}});
            if(name==='body_timeout'){
                res.writeHead(200,{'content-type':'application/json','content-length':Buffer.byteLength(data)});
                res.write(data.slice(0,100));holdOpen();return;
            }
            if(name==='sse'){
                res.writeHead(200,{'content-type':'text/event-stream'});
                res.flushHeaders();
                holdOpen();
                const event='event: message\r\ndata: '+data;
                const sizes=[1,2,7,3,4093,16381];let offset=0,fragments=0;
                while(offset<event.length){
                    const fragment=event.slice(offset,offset+sizes[fragments%sizes.length]);
                    if(!res.write(fragment))await new Promise(resolve=>res.once('drain',resolve));
                    offset+=fragment.length;fragments++;
                    if(fragments<8)await new Promise(resolve=>setTimeout(resolve,2));
                }
                for(const fragment of ['\r','\n\r','\n']){res.write(fragment);await new Promise(resolve=>setTimeout(resolve,5));}
                log({case:name,stage:'event_written',bytes:Buffer.byteLength(data),fragments,protocol,alpn_h2});
                return;
            }
            reply(200,data);log({case:name,stage:'body_written',bytes:Buffer.byteLength(data),protocol,alpn_h2});
        });
    });
    server.listen(listenPort,'0.0.0.0',()=>log({stage:'ready',port:listenPort,wrong_root:Number(wrongRoot),tools:tools.length}));
}
serve('',port,false);
serve('wrong-',wrongPort,true);
