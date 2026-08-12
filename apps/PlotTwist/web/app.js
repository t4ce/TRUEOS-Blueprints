/* Preset art: Twemoji 14.0.2, Copyright Twitter, Inc. and contributors, CC-BY 4.0. */
const $=id=>document.getElementById(id);
const faces=['1f600','1f60e','1f929','1f914','1f920'];
const faceUrl=i=>`/emoji/${faces[i]}`;
let selected={kind:'preset',value:0},session=null,snapshot=null,pollTimer=0,serverOffset=0;
const camera={x:0,y:520,z:-650,yaw:0,pitch:-.56},keys=new Set();
let dragging=false,lastPointer=[0,0],downPointer=[0,0],lastFrame=performance.now();

faces.forEach((_,i)=>{const b=document.createElement('button');b.className='preset'+(i===0?' selected':'');b.innerHTML=`<img alt="preset ${i+1}" src="${faceUrl(i)}">`;b.onclick=()=>{selected={kind:'preset',value:i};document.querySelectorAll('.preset').forEach(x=>x.classList.remove('selected'));b.classList.add('selected');$('avatarPreview').classList.add('hidden')};$('presets').append(b)});

$('avatarFile').onchange=async e=>{const file=e.target.files[0];if(!file)return;try{const image=await createImageBitmap(file);const canvas=document.createElement('canvas');canvas.width=canvas.height=64;const ctx=canvas.getContext('2d');const side=Math.min(image.width,image.height),sx=(image.width-side)/2,sy=(image.height-side)/2;ctx.drawImage(image,sx,sy,side,side,0,0,64,64);selected={kind:'image',value:canvas.toDataURL('image/webp',.82)};$('avatarPreview').src=selected.value;$('avatarPreview').classList.remove('hidden');document.querySelectorAll('.preset').forEach(x=>x.classList.remove('selected'));image.close()}catch{$('connectError').textContent='Could not read that image.'}};

async function api(path,body,method='POST'){
  const options={method,headers:{}};if(body!==undefined){options.headers['content-type']='application/json';options.body=JSON.stringify(body)}
  const response=await fetch(path,options),data=await response.json();if(!response.ok||!data.ok)throw new Error(data.error||'Request failed');return data;
}
function show(id){['connect','browser','room','game'].forEach(x=>$(x).classList.toggle('hidden',x!==id))}
function avatarSource(avatar){return avatar.kind==='preset'?faceUrl(avatar.value):avatar.value}
function errorAt(id,error){$(id).textContent=error?.message||String(error||'')}

$('connectButton').onclick=async()=>{errorAt('connectError','');try{const data=await api('/api/connect',{nickname:$('nickname').value,avatar:selected});session=data.session;show('browser');await refreshLobbies()}catch(e){errorAt('connectError',e)}};
$('createButton').onclick=async()=>{try{enterLobby((await api('/api/lobbies',{token:session.token})).snapshot)}catch(e){errorAt('browserError',e)}};
async function refreshLobbies(){try{const data=await api('/api/lobbies',undefined,'GET');serverOffset=data.serverNowMs-performance.now();$('lobbies').replaceChildren(...data.lobbies.map(lobby=>{const row=document.createElement('div');row.className='lobby-row';row.innerHTML=`<div><strong>${escapeHtml(lobby.name)}</strong><small> ${lobby.id} · ${lobby.playerCount}/4 · ${lobby.phase}</small></div>`;const join=document.createElement('button');join.textContent='Join';join.onclick=async()=>{try{enterLobby((await api(`/api/lobbies/${lobby.id}/join`,{token:session.token})).snapshot)}catch(e){errorAt('browserError',e)}};row.append(join);return row}));if(!data.lobbies.length)$('lobbies').innerHTML='<p class="hint">No open lobby yet. Make the first plot.</p>'}catch(e){errorAt('browserError',e)}}
function enterLobby(value){snapshot=value;show(value.game?'game':'room');render();schedulePoll()}
function schedulePoll(){clearTimeout(pollTimer);if(!snapshot)return;pollTimer=setTimeout(poll,500)}
async function poll(){if(!snapshot)return;try{snapshot=(await api(`/api/lobbies/${snapshot.id}/state`,{token:session.token})).snapshot;serverOffset=snapshot.serverNowMs-performance.now();render()}catch(e){errorAt(snapshot.game?'gameError':'roomError',e)}schedulePoll()}
async function action(action){if(!snapshot)return;const data=await api(`/api/lobbies/${snapshot.id}/action`,{token:session.token,action});if(data.result.left){snapshot=null;show('browser');await refreshLobbies()}else{snapshot=data.result.snapshot;render()}}

function render(){if(!snapshot)return;serverOffset=snapshot.serverNowMs-performance.now();if(snapshot.game){show('game');renderGameUi()}else{show('room');renderRoom()}}
function renderRoom(){
  $('lobbyId').textContent=snapshot.id;$('lobbyName').textContent=snapshot.name;const me=snapshot.players.find(p=>p.playerId===session.playerId);const phase=snapshot.phase.kind;
  let badge=phase;if(phase==='countdown')badge=`Starts in ${Math.max(0,Math.ceil((snapshot.phase.endsAtMs-now())/1000))}s`;if(phase==='finished')badge=snapshot.phase.reason;$('phaseBadge').textContent=badge;
  $('players').replaceChildren(...snapshot.players.map(player=>{const row=document.createElement('div');row.className='player-row';row.innerHTML=`<div class="player-id"><img class="avatar" src="${avatarSource(player.avatar)}"><div><strong>${escapeHtml(player.nickname)}</strong><small> · ${player.ready?'READY':'not ready'} · ${player.aliveFigures} faces</small></div></div>`;const controls=document.createElement('div');controls.className='player-controls';if(player.playerId===session.playerId&&(phase==='waiting'||phase==='countdown')){const team=document.createElement('select');team.innerHTML=[1,2,3,4].map(n=>`<option ${player.team===n?'selected':''}>${n}</option>`).join('');team.onchange=()=>safeAction({type:'setTeam',team:+team.value},'roomError');const color=document.createElement('input');color.type='color';color.value=player.color;color.onchange=()=>safeAction({type:'setColor',color:color.value},'roomError');controls.append('Team ',team,color)}else{controls.textContent=`Team ${player.team}`;controls.style.color=player.color}row.append(controls);return row}));
  $('readyButton').disabled=phase==='finished';$('readyButton').textContent=me?.ready?'Stop / reset':'Ready';$('readyButton').onclick=()=>safeAction({type:'setReady',ready:!me?.ready},'roomError');
  if(phase==='countdown')setTimeout(()=>snapshot&&!snapshot.game&&renderRoom(),200);
}
$('leaveButton').onclick=()=>safeAction({type:'leave'},'roomError');
async function safeAction(value,errorId){errorAt(errorId,'');try{await action(value)}catch(e){errorAt(errorId,e)}}

function renderGameUi(){
  const game=snapshot.game,active=snapshot.players.find(p=>p.playerId===game.currentPlayerId),mine=game.currentPlayerId===session.playerId;$('turnLabel').textContent=`Round ${game.round} · ${active?.nickname||'—'}${mine?' (you)':''}`;$('pauseButton').textContent=game.paused?'Resume':'Pause';
  const chat=$('chat'),atBottom=chat.scrollHeight-chat.scrollTop-chat.clientHeight<25;chat.innerHTML=snapshot.chat.map(m=>`<div class="message"><b>${escapeHtml(m.nickname)}</b> ${escapeHtml(m.text)}</div>`).join('');if(atBottom)chat.scrollTop=chat.scrollHeight;
  const me=snapshot.players.find(p=>p.playerId===session.playerId);$('measurements').innerHTML=(me?.measurements||[]).map((p,i)=>`<span class="measurement">${i+1}: ${p.x.toFixed(1)}, ${p.y.toFixed(1)}</span>`).join('');$('expression').disabled=!mine||game.paused;$('plotForm').querySelector('button').disabled=!mine||game.paused;
}
$('pauseButton').onclick=()=>safeAction({type:'pause',paused:!snapshot.game.paused},'gameError');$('endButton').onclick=()=>{if(confirm('End this game for everybody?'))safeAction({type:'endGame'},'gameError')};
$('chatForm').onsubmit=e=>{e.preventDefault();const text=$('chatInput').value;if(text.trim()){safeAction({type:'chat',text},'gameError');$('chatInput').value=''}};
$('plotForm').onsubmit=e=>{e.preventDefault();safeAction({type:'plot',expression:$('expression').value},'gameError')};

const canvas=$('world'),ctx=canvas.getContext('2d');
function resize(){const d=Math.min(devicePixelRatio||1,2);canvas.width=innerWidth*d;canvas.height=innerHeight*d;canvas.style.width=innerWidth+'px';canvas.style.height=innerHeight+'px';ctx.setTransform(d,0,0,d,0,0)}addEventListener('resize',resize);resize();
function basis(){const cy=Math.cos(camera.yaw),sy=Math.sin(camera.yaw),cp=Math.cos(camera.pitch),sp=Math.sin(camera.pitch);return{f:[sy*cp,sp,cy*cp],r:[cy,0,-sy],u:[-sy*sp,cp,-cy*sp]}}
function project(p){const b=basis(),d=[p[0]-camera.x,p[1]-camera.y,p[2]-camera.z],depth=dot(d,b.f);if(depth<2)return null;const f=Math.min(innerWidth,innerHeight)*.82;return[innerWidth/2+dot(d,b.r)/depth*f,innerHeight/2-dot(d,b.u)/depth*f,depth]}
function dot(a,b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2]}
function line(a,b,color='#333943',width=1){const x=project(a),y=project(b);if(!x||!y)return;ctx.beginPath();ctx.moveTo(x[0],x[1]);ctx.lineTo(y[0],y[1]);ctx.strokeStyle=color;ctx.lineWidth=width;ctx.stroke()}
function polygon(points,fill,stroke){const q=points.map(project);if(q.some(x=>!x))return;ctx.beginPath();ctx.moveTo(q[0][0],q[0][1]);q.slice(1).forEach(p=>ctx.lineTo(p[0],p[1]));ctx.closePath();ctx.fillStyle=fill;ctx.fill();ctx.strokeStyle=stroke;ctx.stroke()}
function drawWorld(){
  ctx.clearRect(0,0,innerWidth,innerHeight);const g=ctx.createLinearGradient(0,0,0,innerHeight);g.addColorStop(0,'#111827');g.addColorStop(1,'#07090c');ctx.fillStyle=g;ctx.fillRect(0,0,innerWidth,innerHeight);
  for(let n=-500;n<=500;n+=50){line([-500,0,n],[500,0,n],n===0?'#677386':'#252a32',n===0?2:1);line([n,0,-500],[n,0,500],n===0?'#677386':'#252a32',n===0?2:1)}line([-500,0,-500],[500,0,-500],'#c8ff46');line([500,0,-500],[500,0,500],'#c8ff46');line([500,0,500],[-500,0,500],'#c8ff46');line([-500,0,500],[-500,0,-500],'#c8ff46');
  if(!snapshot?.game)return;const game=snapshot.game;
  [...game.obstacles].sort((a,b)=>distance(b)-distance(a)).forEach(o=>{const x=o.x,w=o.width/2,z=o.y,d=o.depth/2,h=o.height;polygon([[x-w,0,z-d],[x+w,0,z-d],[x+w,h,z-d],[x-w,h,z-d]],'#343840','#666b75');polygon([[x+w,0,z-d],[x+w,0,z+d],[x+w,h,z+d],[x+w,h,z-d]],'#272b32','#666b75');polygon([[x-w,h,z-d],[x+w,h,z-d],[x+w,h,z+d],[x-w,h,z+d]],'#4b505a','#7b818c')});
  game.traces.forEach(t=>{const player=snapshot.players.find(p=>p.playerId===t.ownerId);ctx.beginPath();let begun=false;t.points.forEach(p=>{const q=project([p.x,2,p.y]);if(!q){begun=false;return}if(!begun){ctx.moveTo(q[0],q[1]);begun=true}else ctx.lineTo(q[0],q[1])});ctx.strokeStyle=player?.color||'#fff';ctx.lineWidth=3;ctx.shadowBlur=12;ctx.shadowColor=ctx.strokeStyle;ctx.stroke();ctx.shadowBlur=0});
  renderLabels(game);
}
function distance(o){return (o.x-camera.x)**2+(o.y-camera.z)**2}
function renderLabels(game){const labels=$('labels'),live=game.figures.filter(f=>f.alive);while(labels.children.length>live.length)labels.lastChild.remove();live.forEach((f,i)=>{let el=labels.children[i];if(!el){el=document.createElement('div');el.className='figure-label';el.innerHTML='<img><span></span>';labels.append(el)}const p=snapshot.players.find(x=>x.playerId===f.ownerId),q=project([f.x,12,f.y]);el.style.display=q?'block':'none';if(q){el.style.left=q[0]+'px';el.style.top=q[1]+'px';el.querySelector('img').src=avatarSource(p.avatar);el.querySelector('img').style.borderColor=p.color;el.querySelector('span').textContent=`${p.nickname} · ${f.index}`}})}
function now(){return performance.now()+serverOffset}
function animate(time){const dt=Math.min((time-lastFrame)/1000,.05);lastFrame=time;const b=basis(),speed=(keys.has('Shift')?350:160)*dt;if(keys.has('w')){camera.x+=b.f[0]*speed;camera.z+=b.f[2]*speed}if(keys.has('s')){camera.x-=b.f[0]*speed;camera.z-=b.f[2]*speed}if(keys.has('a')){camera.x-=b.r[0]*speed;camera.z-=b.r[2]*speed}if(keys.has('d')){camera.x+=b.r[0]*speed;camera.z+=b.r[2]*speed}if(keys.has('q'))camera.y=Math.max(5,camera.y-speed);if(keys.has('e'))camera.y=Math.min(1000,camera.y+speed);camera.x=Math.max(-850,Math.min(850,camera.x));camera.z=Math.max(-850,Math.min(850,camera.z));drawWorld();if(snapshot?.game){const remain=snapshot.game.paused?null:Math.max(0,snapshot.game.turnEndsAtMs-now());$('timer').textContent=remain===null?'PAUSED':`${Math.floor(remain/60000)}:${String(Math.ceil(remain/1000)%60).padStart(2,'0')}`}requestAnimationFrame(animate)}requestAnimationFrame(animate);
addEventListener('keydown',e=>{if(!/^(INPUT|SELECT|TEXTAREA)$/.test(e.target.tagName))keys.add(e.key.toLowerCase())});addEventListener('keyup',e=>keys.delete(e.key.toLowerCase()));canvas.onpointerdown=e=>{dragging=true;downPointer=lastPointer=[e.clientX,e.clientY];canvas.setPointerCapture(e.pointerId)};canvas.onpointermove=e=>{if(!dragging)return;camera.yaw-=(e.clientX-lastPointer[0])*.005;camera.pitch=Math.max(-1.45,Math.min(.25,camera.pitch-(e.clientY-lastPointer[1])*.004));lastPointer=[e.clientX,e.clientY]};canvas.onpointerup=e=>{const moved=Math.hypot(e.clientX-downPointer[0],e.clientY-downPointer[1]);dragging=false;if(moved<4)measureAt(e.clientX,e.clientY)};
function measureAt(sx,sy){if(!snapshot?.game||snapshot.game.currentPlayerId!==session.playerId||snapshot.game.paused)return;const b=basis(),f=Math.min(innerWidth,innerHeight)*.82,nx=(sx-innerWidth/2)/f,ny=-(sy-innerHeight/2)/f,ray=[b.f[0]+b.r[0]*nx+b.u[0]*ny,b.f[1]+b.r[1]*nx+b.u[1]*ny,b.f[2]+b.r[2]*nx+b.u[2]*ny];if(ray[1]>=-.001)return;const t=-camera.y/ray[1],x=camera.x+ray[0]*t,y=camera.z+ray[2]*t;if(x>=-500&&x<=500&&y>=-500&&y<=500)safeAction({type:'measure',x,y},'gameError')}
function escapeHtml(value){return String(value).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]))}
setInterval(()=>{if(session&&!$('browser').classList.contains('hidden'))refreshLobbies()},2500);
