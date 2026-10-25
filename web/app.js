'use strict';
/* Five Trails browser client.
   Lobby data (profile, trophies, XP, records, settings) is kept in this browser's IndexedDB, and a backup of it goes to the
   server so a new phone can restore it. Everything with real value (ranked results, coins, free chests, quests, seasons)
   lives on the server, which also runs every online match. Your secret account key (the recovery code) proves who you are. */
const IDB={db:null,
 open(){return new Promise(res=>{let r;try{r=indexedDB.open('five-trails',1)}catch(e){res(null);return}
  r.onupgradeneeded=()=>{try{r.result.createObjectStore('kv')}catch(e){}};
  r.onsuccess=()=>{IDB.db=r.result;res(r.result)};r.onerror=()=>res(null);r.onblocked=()=>res(null)})},
 loadAll(){return new Promise(res=>{const out={};if(!this.db){res(out);return}
  try{const req=this.db.transaction('kv','readonly').objectStore('kv').openCursor();
   req.onsuccess=()=>{const c=req.result;if(c){out[c.key]=c.value;c.continue()}else res(out)};req.onerror=()=>res(out)}catch(e){res(out)}})},
 put(k,v){if(!this.db)return;try{this.db.transaction('kv','readwrite').objectStore('kv').put(v,k)}catch(e){}}};
const ST={c:{},get(k,d){return Object.prototype.hasOwnProperty.call(this.c,k)?this.c[k]:d},set(k,v){this.c[k]=v;IDB.put(k,v)}};
const SS={get(k){try{return sessionStorage.getItem(k)}catch(e){return null}},set(k,v){try{v==null?sessionStorage.removeItem(k):sessionStorage.setItem(k,v)}catch(e){}}};

(async function main(){
await IDB.open();
ST.c=await IDB.loadAll();
/* ================= Data ================= */
const COL=[
 {n:'Desert', cc:'#ffd23a',cd:'#ff9f0a',ct:'#3a2200',i:'<circle cx="8" cy="8" r="3.4"/><path d="M8 .8v2.4M8 12.8v2.4M.8 8h2.4M12.8 8h2.4M2.9 2.9l1.7 1.7M11.4 11.4l1.7 1.7M13.1 2.9l-1.7 1.7M4.6 11.4l-1.7 1.7" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>'},
 {n:'Sea',    cc:'#3aa0ff',cd:'#0a5cf0',ct:'#ffffff',i:'<path d="M.8 6.2c2.2-2.2 3.8-2.2 5.9 0s3.8 2.2 5.9 0c.9-.9 1.6-1.2 2.6-1.2M.8 11.2c2.2-2.2 3.8-2.2 5.9 0s3.8 2.2 5.9 0c.9-.9 1.6-1.2 2.6-1.2" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" fill="none"/>'},
 {n:'Jungle', cc:'#3df07c',cd:'#0fae4a',ct:'#03300f',i:'<path d="M2.2 13.8C2.2 6 6.8 2.2 14 2.2 14 9.4 9.8 13.8 2.2 13.8Z"/><path d="M2.6 13.4 10 6" stroke="var(--cc)" stroke-width="1.4" stroke-linecap="round"/>'},
 {n:'Volcano',cc:'#ff5a48',cd:'#e0170a',ct:'#ffffff',i:'<path d="M.8 14.6 5.8 5.2h4.4l5 9.4Z"/><path d="M6.8 3.6 6 1M9.2 3.6 10 1M8 3.2V.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>'},
 {n:'Glacier',cc:'#ffffff',cd:'#c9e6ff',ct:'#0b4f8a',i:'<path d="M8 .8v14.4M1.8 4.4l12.4 7.2M14.2 4.4 1.8 11.6M8 3.2 6.4 1.8M8 3.2l1.6-1.4M8 12.8l-1.6 1.4M8 12.8l1.6 1.4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" fill="none"/>'}
];
const ARENAS=[
 {n:'Base Camp',at:0,g:['#c9d7f5','#6a80ba'],ic:'<path d="M8 1.5 15.2 14H.8Z"/><path d="M8 6.5 10.6 14H5.4Z" fill="#6a80ba"/>'},
 {n:'Sunscorch Dunes',at:100,g:['#ffe066','#f08a00'],c:0},
 {n:'Coral Shallows',at:250,g:['#6cc0ff','#1a5fe0'],c:1},
 {n:'Emerald Canopy',at:450,g:['#6ff59a','#139a45'],c:2},
 {n:'Ember Caldera',at:700,g:['#ff8a6a','#c81e10'],c:3},
 {n:'Frostcrown Pass',at:1000,g:['#e9f6ff','#7fb6ec'],c:4,dark:true},
 {n:'Summit of Legends',at:1400,g:['#ffe27a','#a24cff'],ic:'<path d="M1 12 2.4 3.6 6 7.6 8 1.6l2 6 3.6-4L15 12Z"/><rect x="1" y="12.8" width="14" height="2.2" rx="1"/>'}
];
const AI_TIER=['Rookie','Apprentice','Skilled','Seasoned','Expert','Master','Legend'];
const AI_NOISE=[8,6,4.5,3.2,2,1,.3];
const EMOTES=['Good luck!','Nice one!','Wow!','Oops!','Hurry up!','Well played!'];
const icon=c=>`<svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">${COL[c].i}</svg>`;
const cC=id=>Math.floor(id/12), cV=id=>{const i=id%12;return i<3?0:i-1};
const cid=(c,v,w=0)=>v?c*12+v+1:c*12+w;
const $=id=>document.getElementById(id);
const esc=s=>String(s??'').replace(/[&<>"']/g,m=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[m]));
const clone=o=>JSON.parse(JSON.stringify(o));
const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
const arenaIdx=t=>{let i=0;ARENAS.forEach((a,j)=>{if(t>=a.at)i=j});return i};
function lvlInfo(xp){let lv=1,need=100,base=0;while(xp>=base+need){base+=need;lv++;need=100+(lv-1)*40}return{lv,into:xp-base,need}}

/* Icons */
const OLC='#0b1640';
const TROPHY=`<svg class="tri" viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3h12v5a6 6 0 0 1-12 0Z" fill="#ffd23a" stroke="${OLC}" stroke-width="1.8" stroke-linejoin="round"/><path d="M6 5H3v1.5A3.5 3.5 0 0 0 6.5 10M18 5h3v1.5a3.5 3.5 0 0 1-3.5 3.5" fill="none" stroke="${OLC}" stroke-width="1.8"/><path d="M10 14h4v3h-4z" fill="#ffaa00" stroke="${OLC}" stroke-width="1.8"/><path d="M7 17h10v4H7z" fill="#ffd23a" stroke="${OLC}" stroke-width="1.8" stroke-linejoin="round"/><path d="M9 5.5v2.5" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/></svg>`;
const LAUREL='<svg viewBox="0 0 100 100" class="laurel" aria-hidden="true"><g fill="#ffd23a" stroke="#8a5a00" stroke-width="1.6"><ellipse cx="18.1" cy="40.4" rx="7" ry="3.4" transform="rotate(255 18.1 40.4)"/><ellipse cx="24.0" cy="30.1" rx="7" ry="3.4" transform="rotate(275 24.0 30.1)"/><ellipse cx="33.0" cy="22.6" rx="7" ry="3.4" transform="rotate(295 33.0 22.6)"/><ellipse cx="44.1" cy="18.5" rx="7" ry="3.4" transform="rotate(315 44.1 18.5)"/><ellipse cx="55.9" cy="18.5" rx="7" ry="3.4" transform="rotate(335 55.9 18.5)"/><ellipse cx="67.0" cy="22.6" rx="7" ry="3.4" transform="rotate(355 67.0 22.6)"/><ellipse cx="76.0" cy="30.1" rx="7" ry="3.4" transform="rotate(375 76.0 30.1)"/><ellipse cx="81.9" cy="40.4" rx="7" ry="3.4" transform="rotate(-75 81.9 40.4)"/><ellipse cx="76.0" cy="30.1" rx="7" ry="3.4" transform="rotate(-95 76.0 30.1)"/><ellipse cx="67.0" cy="22.6" rx="7" ry="3.4" transform="rotate(-115 67.0 22.6)"/><ellipse cx="55.9" cy="18.5" rx="7" ry="3.4" transform="rotate(-135 55.9 18.5)"/><ellipse cx="44.1" cy="18.5" rx="7" ry="3.4" transform="rotate(-155 44.1 18.5)"/><ellipse cx="33.0" cy="22.6" rx="7" ry="3.4" transform="rotate(-175 33.0 22.6)"/><ellipse cx="24.0" cy="30.1" rx="7" ry="3.4" transform="rotate(-195 24.0 30.1)"/></g><path d="M50 30l5.5 11.2 12.3 1.8-8.9 8.7 2.1 12.3L50 58.2l-11 5.8 2.1-12.3-8.9-8.7 12.3-1.8z" fill="#fff3b0" stroke="#8a5a00" stroke-width="2"/></svg>';
const SV=(p,f='none',w=2.6)=>`<svg viewBox="0 0 24 24" fill="${f}" stroke="currentColor" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${p}</svg>`;
const EYEMINI=`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12Z"/><circle cx="12" cy="12" r="3" fill="currentColor"/></svg>`;
const CUPIC=`<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2.5 4.5h5v5h3.5M2.5 19.5h5v-5h3.5M11 9.5v5M11 12h4"/><rect x="15" y="9" width="6.5" height="6" rx="1.5"/></svg>`;
const IC={
 swords:SV('<path d="M4 4l9.5 9.5M4 4h4.5M4 4v4.5M20 4l-9.5 9.5M20 4h-4.5M20 4v4.5M6.5 15.5l2 2M17.5 15.5l-2 2M5 19l2.5-2.5M19 19l-2.5-2.5"/>'),
 cup:SV('<path d="M7 4h10v4a5 5 0 0 1-10 0Z M7 6H4v1a3 3 0 0 0 3 3 M17 6h3v1a3 3 0 0 1-3 3 M12 13v4 M8 20h8"/>'),
 book:SV('<path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15H6.5A2.5 2.5 0 0 0 4 20.5Z M4 20.5V5.5 M8.5 7.5h7"/>'),
 crown:SV('<path d="M3.5 18.5 2.5 7l5 4.5L12 4l4.5 7.5 5-4.5-1 11.5Z M4.5 21.5h15"/>'),
 lock:SV('<rect x="5" y="11" width="14" height="9.5" rx="2.2" fill="#fff"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>','none',2.6),
 gear:SV('<circle cx="12" cy="12" r="3.2"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1"/>'),
 x:SV('<path d="M6 6l12 12M18 6 6 18"/>','none',3.4),
 chat:SV('<path d="M4 5h16v11H10l-5 4v-4H4Z"/>','#fff',2.2),
 sndOn:SV('<path d="M4 9h4l5-4v14l-5-4H4Z" fill="#fff"/><path d="M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12"/>'),
 sndOff:SV('<path d="M4 9h4l5-4v14l-5-4H4Z" fill="#fff"/><path d="M16.5 9.5l5 5M21.5 9.5l-5 5"/>'),
 clock:`<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="13" r="8.5" fill="#fff" stroke="${OLC}" stroke-width="2.2"/><path d="M12 8.5V13l3 2" fill="none" stroke="${OLC}" stroke-width="2.2" stroke-linecap="round"/><path d="M9.5 2.5h5" stroke="${OLC}" stroke-width="2.4" stroke-linecap="round"/></svg>`,
 bot:`<svg viewBox="0 0 16 16" aria-hidden="true"><rect x="2.5" y="4.5" width="11" height="9" rx="2.8" fill="#fff" stroke="${OLC}" stroke-width="1.3"/><circle cx="6" cy="9" r="1.3" fill="${OLC}"/><circle cx="10" cy="9" r="1.3" fill="${OLC}"/><path d="M8 4.5V2.2" stroke="${OLC}" stroke-width="1.3"/><circle cx="8" cy="1.8" r="1.1" fill="#ffd23a" stroke="${OLC}" stroke-width=".8"/></svg>`
};
const AVC=[['#ff9a6c','#e6452a'],['#68cdff','#1f7be0'],['#7cf08a','#22a83e'],['#c79aff','#7a3ae6'],['#ffd65c','#e69a00'],['#ff86bf','#d6286f']];
function hueOf(s){let h=7;for(const c of String(s))h=(h*31+c.charCodeAt(0))>>>0;return h}
function avatarHTML(name,lv,cpu){const [a,b]=cpu?['#a9bce3','#5b6fa6']:AVC[hueOf(name)%AVC.length];const ch=((name||'?').trim()[0]||'?').toUpperCase();
 return `<div class="av" style="--a1:${a};--a2:${b}">${cpu?IC.bot:esc(ch)}${lv?`<span class="lvb">${lv}</span>`:''}</div>`}
let embN=0;
function emblem(i,size=120,cls=''){const A=ARENAS[i],id='eg'+(embN++);const ic=A.ic||COL[A.c].i;const fg=A.dark?'#1d5fa6':'#fff';const sh='M60 8 106 23v38c0 34-21 53-46 63C35 114 14 95 14 61V23Z';
 return `<svg class="${cls}" viewBox="0 0 120 136" width="${size}" height="${Math.round(size*1.13)}" aria-hidden="true"><defs><linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${A.g[0]}"/><stop offset="1" stop-color="${A.g[1]}"/></linearGradient></defs>
 <path d="${sh}" fill="${OLC}" transform="translate(0 6)"/><path d="${sh}" fill="url(#${id})" stroke="${OLC}" stroke-width="5" stroke-linejoin="round"/>
 <path d="M60 19 95 30.5V61c0 26-15.5 41.5-35 49.5C40.5 102.5 25 87 25 61V30.5Z" fill="rgba(255,255,255,.2)"/>
 <g transform="translate(32 34) scale(3.5)" fill="${fg}" color="${fg}" style="--cc:${A.g[1]}">${ic}</g>
 <path d="M28 25 60 14.5 92 25" stroke="rgba(255,255,255,.65)" stroke-width="4" fill="none" stroke-linecap="round"/></svg>`}
function cardHTML(id,extra='',style=''){const c=cC(id),v=cV(id),C=COL[c];
 return `<div class="card ${v?'':'w'} ${v>=10?'dd':''} ${extra}" data-id="${id}" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct};${style}" aria-label="${C.n} ${v||'wager'}"><span class="cv">${v||'×'}</span><span class="ci">${icon(c)}</span>${v?`<span class="art">${icon(c)}</span>`:'<span class="ring"></span>'}</div>`}
const mini=(id,w=40,st='')=>cardHTML(id,'',`width:${w}px;height:${Math.round(w*1.4)}px;${st}`).replace('<span class="cv">',`<span class="cv" style="font-size:${Math.round(w*.48)}px">`);

/* ================= Engine ================= */
const rid=()=>Math.random().toString(36).slice(2,9);
const VS_MS=2800, RUSH_MS=10000;
function deal(first,base){const d=[...Array(60).keys()];for(let i=59;i>0;i--){const j=Math.floor(Math.random()*(i+1));[d[i],d[j]]=[d[j],d[i]]}
 const t0=Date.now()+VS_MS;
 return{deck:d,hands:[d.splice(0,8),d.splice(0,8)],exp:[[[],[],[],[],[]],[[],[],[],[],[]]],disc:[[],[],[],[],[]],turn:first,phase:'play',jd:-1,over:false,seq:1,gid:rid(),last:null,flag:null,why:null,t0,
  pw:{sab:[1,1],rush:[1,1]},rushNext:null,rushUntil:null,rushSeat:null,rushRem:null,clk:base?{base,left:[base,base],since:t0,paused:false}:null}}
function clockSpend(S,now){const k=S.clk;if(k&&!k.paused){k.left[S.turn]-=Math.max(0,now-k.since);k.since=Math.max(now,k.since)}}
function clockLeft(S,s,now){const k=S.clk;if(!k)return null;let l=k.left[s];if(S.turn===s&&!S.over&&!k.paused)l-=Math.max(0,now-k.since);return Math.max(0,l)}
function clockPause(S,now){if(S.clk&&!S.clk.paused){clockSpend(S,now);S.clk.paused=true}if(S.rushUntil&&S.rushRem==null)S.rushRem=Math.max(0,S.rushUntil-now)}
function clockResume(S,now){if(S.clk&&S.clk.paused){S.clk.paused=false;S.clk.since=Math.max(now,S.clk.since)}if(S.rushRem!=null){S.rushUntil=now+S.rushRem;S.rushRem=null}}
function ensurePw(S){if(!S.pw)S.pw={sab:[1,1],rush:[1,1]};if(!S.eyeUsed)S.eyeUsed=[false,false];if(!S.exposed)S.exposed=[null,null];if(!S.turnsDone)S.turnsDone=[0,0];if(!S.eyeLog)S.eyeLog=[null,null]}
function okSab(S,s){ensurePw(S);return !S.over&&S.turn===s&&S.phase==='play'&&S.pw.sab[s]>0&&S.deck.length>=3&&S.hands[1-s].length>0}
function okEye(S,s){ensurePw(S);return !S.over&&S.turn===s&&S.phase==='play'&&!S.eyeUsed[s]&&S.deck.length>=10&&S.hands[1-s].length>0&&!(S.rushSeat===s&&S.rushUntil)}
function doEye(S,s){const v=1-s,h=S.hands[v],id=h[Math.floor(Math.random()*h.length)];S.eyeUsed[s]=true;S.exposed[v]=id;S.eyeLog[s]=[id,S.turnsDone[s]+1];S.seq++;S.last={seq:S.seq,seat:s,k:'eye',id,p:cC(id)}}
function doSab(S,s){const v=1-s,h=S.hands[v],id=h[Math.floor(Math.random()*h.length)];h.splice(h.indexOf(id),1);S.disc[cC(id)].push(id);const r=S.deck.pop();h.push(r);S.pw.sab[s]--;S.seq++;S.last={seq:S.seq,seat:s,k:'sab',id,p:cC(id),r}}
function okRush(S,s){ensurePw(S);return !S.over&&S.turn===s&&S.pw.rush[s]>0&&S.rushNext==null}
function doRush(S,s){S.pw.rush[s]--;S.rushNext=1-s;S.seq++;S.last={seq:S.seq,seat:s,k:'rush',id:-1,p:-1}}
function autoMove(S,s){if(S.phase==='play'){const h=S.hands[s];doPlay(S,s,h[Math.floor(Math.random()*h.length)],false)}doDraw(S,s,-1);S.last.auto=true}
function doFlag(S,s,why='time'){if(S.clk){clockSpend(S,Date.now());if(why==='time')S.clk.left[s]=0}S.over=true;S.flag=s;S.why=why;S.rushUntil=null;S.rushSeat=null;S.rushRem=null;S.seq++;S.last={seq:S.seq,seat:s,k:'flag',id:-1,p:-1}}
function canE(e,id){if(!e.length)return true;const t=cV(e[e.length-1]),v=cV(id);return v===0?t===0:v>t}
function whyNot(e,id){const t=cV(e[e.length-1]);return cV(id)===0?'Wagers go before any numbered card.':`That card must be higher than your ${t||'wager'}.`}
function okPlay(S,s,id,x){return !S.over&&S.turn===s&&S.phase==='play'&&S.hands[s].includes(id)&&(!x||canE(S.exp[s][cC(id)],id))}
function doPlay(S,s,id,x){const h=S.hands[s];h.splice(h.indexOf(id),1);const c=cC(id);
 if(x){S.exp[s][c].push(id);S.jd=-1}else{S.disc[c].push(id);S.jd=c}S.phase='draw';S.seq++;S.last={seq:S.seq,seat:s,k:x?'play':'disc',id,p:c}}
function okDraw(S,s,p){return !S.over&&S.turn===s&&S.phase==='draw'&&(p<0?S.deck.length>0:(p!==S.jd&&S.disc[p].length>0))}
function doDraw(S,s,p){ensurePw(S);S.turnsDone[s]++;clockSpend(S,Date.now());const id=p<0?S.deck.pop():S.disc[p].pop();S.hands[s].push(id);S.jd=-1;S.phase='play';
 if(!S.deck.length)S.over=true;else S.turn=1-s;S.seq++;S.last={seq:S.seq,seat:s,k:'draw',id,p};
 if(S.rushSeat===s||S.over){S.rushUntil=null;S.rushSeat=null;S.rushRem=null}
 if(!S.over&&S.rushNext===S.turn){S.rushUntil=Date.now()+RUSH_MS;S.rushSeat=S.turn;S.rushNext=null}}
function viewFor(S,s){const L=S.last;ensurePw(S);const now=Date.now();return clone({
 pw:{sab:[S.pw.sab[s],S.pw.sab[1-s]],eye:[S.eyeUsed[s]?0:1,S.eyeUsed[1-s]?0:1],rush:[S.pw.rush[s],1]},
 el:[S.eyeLog[s],S.eyeLog[1-s]],
 ey:S.exposed[1-s]!=null&&S.hands[1-s].includes(S.exposed[1-s])?S.exposed[1-s]:-1,em:S.exposed[s]!=null&&S.hands[s].includes(S.exposed[s])?S.exposed[s]:-1,
 ru:S.rushUntil&&!S.over?{seat:S.rushSeat===s?0:1,l:S.rushRem!=null?S.rushRem:S.rushUntil-now,pz:S.rushRem!=null}:null,
 rn:S.rushNext==null||S.rushNext===s?null:1,gid:S.gid,seq:S.seq,me:S.hands[s],on:S.hands[1-s].length,ex:[S.exp[s],S.exp[1-s]],di:S.disc,dn:S.deck.length,
 mt:S.turn===s,ph:S.phase,jd:S.jd,ov:S.over,why:S.why,fl:S.flag==null?null:(S.flag===s?'me':'opp'),
 ck:S.clk?{b:S.clk.base,l:[clockLeft(S,s,now),clockLeft(S,1-s,now)],run:S.over||S.clk.paused?null:(S.turn===s?0:1),d:Math.max(0,S.clk.since-now)}:null,
 la:L&&{seq:L.seq,by:L.seat===s?'me':'opp',k:(L.k==='rush'&&L.seat!==s)?'none':L.k,id:(L.k==='draw'&&L.p<0&&L.seat!==s)?-1:L.id,p:L.p,r:(L.k==='sab'&&L.seat!==s)?L.r:-1,auto:!!L.auto}})}
function expScore(e){if(!e.length)return 0;let s=0,w=0;for(const id of e){const v=cV(id);if(v)s+=v;else w++}return(s-20)*(1+w)+(e.length>=8?20:0)}
const total=ex=>ex.reduce((a,e)=>a+expScore(e),0);
const lastNum=e=>{let m=1;for(const id of e)m=Math.max(m,cV(id));return m};

/* ================= Computer opponent ================= */
function aiChoosePlay(S,nz=0){const s=1,h=S.hands[s],left=S.deck.length;let best=null;
 const take=(sc,id,x)=>{sc+=(Math.random()*2-1)*nz;if(!best||sc>best.sc)best={sc,id,x}};
 for(const id of h){const c=cC(id),v=cV(id),e=S.exp[s][c],same=h.filter(k=>cC(k)===c&&k!==id);const future=Math.min(left,30)/30*9;
  if(canE(e,id)){
   if(v===0){const pot=same.filter(k=>cV(k)).reduce((a,k)=>a+cV(k),0);take(pot+future-20>=6&&left>18?4:-12,id,true)}
   else if(e.length){const ln=lastNum(e);const skipped=same.filter(k=>cV(k)>ln&&cV(k)<v).length;take(9-(v-ln-1)*.9-skipped*5,id,true)}
   else{const pot=v+same.filter(k=>cV(k)>v).reduce((a,k)=>a+cV(k),0);const wag=same.some(k=>cV(k)===0);const proj=pot+future-20;
    take((proj>0?proj*.45:proj*.8)-(v-2)*.4-(wag?3:0)-(left<10?4:0),id,true)}
  }
  let d=1;const oe=S.exp[0][c];
  if(v&&canE(oe,id))d-=oe.length?6+(v>=8?2:0):1.5;
  if(e.length&&v>lastNum(e))d-=5;
  if(v===0&&!e.length&&same.filter(k=>cV(k)).length>=3)d-=3;
  if(!e.length&&v&&!same.length)d+=1.5;
  take(d-.01*v,id,false)}
 return best}
function aiChooseDraw(S){const s=1;let pick=-1,ps=0;
 for(let c=0;c<5;c++){if(c===S.jd||!S.disc[c].length)continue;const t=S.disc[c][S.disc[c].length-1],e=S.exp[s][c];
  if(e.length&&canE(e,t)){const sc=6-(cV(t)-lastNum(e));if(sc>ps){ps=sc;pick=c}}}
 if(pick<0&&S.deck.length<=3&&total(S.exp[1])>total(S.exp[0]))for(let c=0;c<5;c++)if(c!==S.jd&&S.disc[c].length){pick=c;break}
 return pick}

/* ================= Sound ================= */
let AC=null,sfxOn=ST.get('ft-sfx',true);
function ac(){if(!AC){try{AC=new(window.AudioContext||window.webkitAudioContext)()}catch(e){return null}}if(AC.state==='suspended')AC.resume().catch(()=>{});return AC}
function tone(f,dur,type='sine',vol=.15,when=0,to){const a=ac();if(!a)return;const t=a.currentTime+when,o=a.createOscillator(),g=a.createGain();
 o.type=type;o.frequency.setValueAtTime(f,t);if(to)o.frequency.exponentialRampToValueAtTime(to,t+dur);
 g.gain.setValueAtTime(0,t);g.gain.linearRampToValueAtTime(vol,t+.008);g.gain.exponentialRampToValueAtTime(.0001,t+dur);o.connect(g).connect(a.destination);o.start(t);o.stop(t+dur+.02)}
function noise(dur,freq,ftype='bandpass',vol=.25,q=1,when=0,to){const a=ac();if(!a)return;const t=a.currentTime+when,n=Math.floor(a.sampleRate*dur),b=a.createBuffer(1,n,a.sampleRate),d=b.getChannelData(0);
 for(let i=0;i<n;i++)d[i]=(Math.random()*2-1)*(1-i/n);const s=a.createBufferSource();s.buffer=b;const f=a.createBiquadFilter();f.type=ftype;f.frequency.setValueAtTime(freq,t);if(to)f.frequency.exponentialRampToValueAtTime(to,t+dur);f.Q.value=q;
 const g=a.createGain();g.gain.value=vol;s.connect(f).connect(g).connect(a.destination);s.start(t)}
const SLO=(fn)=>()=>{const T=1.125;const t0=tone,n0=noise;return fn((f,d,ty,v,w=0,g)=>t0(f,d*T,ty,v,w*T,g),(d,f,ty,v,q,w=0,g)=>n0(d*T,f,ty,v,q,w*T,g))};
const SFX={
 ui:()=>{tone(660,.05,'triangle',.08);tone(990,.07,'triangle',.06,.035)},
 select:()=>tone(880,.05,'triangle',.06),
 swoosh:()=>noise(.3,400,'bandpass',.22,1.2,0,3200),
 vs:()=>{noise(.3,6000,'bandpass',.18,1,0,300);tone(68,.7,'sine',.5,.28,38);noise(.45,700,'lowpass',.4,1,.28);tone(392,.5,'sawtooth',.05,.3);tone(523,.55,'sawtooth',.045,.3);tone(784,.6,'triangle',.06,.3)},
 go:()=>{tone(523,.1,'square',.05);tone(784,.22,'square',.05,.09);tone(1046,.3,'triangle',.08,.18)},
 coin:()=>{tone(1568,.05,'square',.035);tone(2093,.12,'sine',.07,.035)},
 levelup:()=>[523,659,784,1046,784,1046,1318].forEach((f,i)=>tone(f,.22,'triangle',.13,i*.08)),
 arena:()=>{tone(130,.9,'sine',.4,0,65);[392,523,659,784,1046].forEach((f,i)=>tone(f,.5,'sawtooth',.04,.1+i*.09));[784,1046,1318].forEach((f,i)=>tone(f,.9,'triangle',.11,.6+i*.12))},
 emote:()=>{tone(700,.07,'sine',.12,0,1100);tone(1100,.09,'sine',.08,.06)},
 play:()=>{noise(.07,2200,'bandpass',.35,.7);tone(170,.14,'sine',.28,0,70)},
 disc:()=>{noise(.16,4200,'bandpass',.3,1.2,0,1500);tone(240,.08,'triangle',.05)},
 draw:()=>{noise(.2,600,'highpass',.18,.7,0,4000)},
 bad:()=>{tone(150,.12,'square',.06);tone(120,.16,'square',.06,.09)},
 turn:()=>{tone(784,.1,'sine',.1);tone(1175,.18,'sine',.1,.09)},
 join:()=>[523,784].forEach((f,i)=>tone(f,.16,'triangle',.12,i*.1)),
 win:()=>{[523,659,784,1046,1318].forEach((f,i)=>tone(f,.3,'triangle',.16,i*.11));[1046,1318,1568].forEach(f=>tone(f,.9,'sine',.06,.6))},
 lose:()=>[440,349,262,196].forEach((f,i)=>tone(f,.38,'triangle',.13,i*.18)),
 deal:()=>{for(let i=0;i<8;i++)noise(.05,3000,'bandpass',.18,1,i*.07)},
 wager:(m=1)=>{noise(.03,7000,'highpass',.2);[2093,2637,3136].forEach((f,i)=>tone(f*m,.45-i*.08,'sine',.11-i*.025,i*.035));tone(1046*m,.5,'triangle',.04,.02)},
 launch:(m=1)=>{tone(392*m,.16,'sawtooth',.05);tone(392*m,.16,'triangle',.12);tone(587*m,.38,'sawtooth',.045,.14);tone(587*m,.38,'triangle',.12,.14)},
 big:(m=1)=>{tone(120*m,.35,'sine',.35,0,48);noise(.12,900,'lowpass',.3);noise(.5,7000,'highpass',.07,1,.04);tone(1568*m,.5,'triangle',.05,.06);tone(2349*m,.45,'sine',.035,.1)},
 positive:(m=1)=>{tone(660*m,.12,'sine',.12,0,990*m);tone(1320*m,.3,'sine',.08,.1)},
 bonus:(m=1)=>[784,988,1175,1568,1976].forEach((f,i)=>{tone(f*m,.22,'triangle',.11,i*.07);tone(f*2*m,.18,'sine',.03,i*.07)}),
 steal:(m=1)=>{noise(.22,500,'bandpass',.22,1.2,0,3000);tone(330*m,.18,'triangle',.12,.08,660*m)},
 dump:(m=1)=>{tone(420*m,.35,'triangle',.11,0,180*m);noise(.18,1200,'lowpass',.15,1,.05)},
 low:()=>{[0,.16].forEach(w=>{tone(1200,.04,'square',.05,w);noise(.03,5000,'highpass',.1,1,w)});tone(520,.25,'sine',.06,.34)},
 tick:()=>{tone(1400,.035,'square',.05);noise(.025,6000,'highpass',.1)},
 sab:(m=1)=>{noise(.35,8000,'bandpass',.25,4,0,300);tone(1800*m,.32,'sawtooth',.06,0,120*m);tone(90,.3,'sine',.3,.18,45)},
 rush:(m=1)=>{noise(.3,300,'bandpass',.22,1,0,6000);[0,.08,.16,.24].forEach((w,i)=>tone((900+i*200)*m,.06,'square',.05,.12+w))},
 alarm:()=>{[0,.14,.28].forEach(w=>{tone(1320,.09,'square',.07,w);tone(990,.09,'square',.05,w+.07)})},
 flag:()=>{tone(220,.7,'sawtooth',.09,0,110);tone(233,.7,'square',.05,0,116)},
 sabx:SLO((tone,noise)=>{const f=n=>440*2**((n-69)/12);
  [0,.4,.8,1.15,1.45,1.7].forEach((w,i)=>tone(f(i%2?41:40),.3,'triangle',.16,w));
  tone(f(52),1.9,'sine',.07,0,f(76));
  [1.9,2.3].forEach(w=>tone(f(91),.22,'triangle',.09,w,f(67)));
  tone(60,.6,'sine',.55,2.9,34);tone(f(28),1.1,'sine',.3,2.9);
  [88,84,80,76,72,68].forEach((n,i)=>tone(f(n),.16,'triangle',.06,3+i*.07));
  [40,47,53].forEach(n=>tone(f(n),.9,'sine',.07,3.1))}),
 eye:SLO((tone,noise)=>{const f=n=>440*2**((n-69)/12);
  tone(f(50),3.9,'sine',.09,0);tone(f(57),3.7,'sine',.06,.2);tone(f(62),3.2,'triangle',.025,.6);
  [62,66,69,73,76,80,81].forEach((n,i)=>tone(f(n),.9,'triangle',.06,.35+i*.16));
  [74,78,81,85].forEach((n,i)=>tone(f(n),.7,'sine',.035,1.6+i*.22));
  [69,73,76,80].forEach((n,i)=>tone(f(n),.5,'triangle',.04,2.25+i*.09));
  [50,57,62,66,69,73,76].forEach(n=>tone(f(n),1.05,n<60?'sine':'triangle',n<60?.12:.05,2.95));
  tone(f(93),1,'sine',.045,3);tone(f(88),.9,'sine',.03,3.12);tone(f(100),.6,'sine',.02,3.2)}),
 last:()=>{for(let i=0;i<14;i++)noise(.06,180+i*12,'lowpass',.22+i*.012,1,i*.045);tone(98,.6,'sine',.35,.66,60);noise(.6,6000,'highpass',.08,1,.66)}
};
const sfx=(k,m)=>{if(sfxOn&&SFX[k])try{SFX[k](m||1)}catch(e){}};
function eventSfx(ev){const opp=ev.by==='opp',m=opp?.8:1,q=[];
 if(ev.k==='flag'){sfx('flag');return}
 if(ev.k==='sab')return;
 if(ev.k==='eye'){return}
 if(ev.k==='rush'){if(!opp)sfx('rush');return}
 if(ev.k==='play'){const e=V.ex[opp?1:0][ev.p],v=cV(ev.id),before=expScore(e.slice(0,-1)),after=expScore(e);sfx('play');
  if(e.length===1)q.push('launch');
  if(v===0)q.push('wager');else if(v>=9)q.push('big');
  if(e.length===8)q.push('bonus');else if(before<=0&&after>0)q.push('positive')}
 else if(ev.k==='disc'){sfx('disc');const v=cV(ev.id);if(v===0||v>=9)q.push('dump')}
 else if(ev.k==='draw'){if(ev.auto)sfx('bad');sfx(ev.p>=0?'steal':'draw');if(V.ru&&V.ru.seat===0&&opp)q.push('alarm');if(V.ov)q.push('last');else if(V.dn===5)q.push('low');else if(V.dn<=3)q.push('tick')}
 q.forEach((k,i)=>setTimeout(()=>sfx(k,m),130+i*280))}
document.addEventListener('pointerdown',()=>ac(),{once:true});

/* ================= Music: lobby theme (C major, 112 bpm, 8-bar loop) ================= */
let musicOn=ST.get('ft-music',true),MUS=null,unlocked=false,NB=null;
const mf=n=>440*Math.pow(2,(n-69)/12),BEAT=60/112;
const CH=[{r:36,t:[60,64,67]},{r:45,t:[57,60,64]},{r:41,t:[57,60,65]},{r:43,t:[55,59,62]},{r:36,t:[60,64,67]},{r:40,t:[55,59,64]},{r:41,t:[57,60,65]},{r:43,t:[55,59,62]}];
const MEL=[[[76,1],[79,1],[81,.5],[79,.5],[76,.5],[74,.5]],[[72,1.5],[74,.5],[76,1],[69,1]],[[77,1],[76,.5],[74,.5],[72,1],[69,1]],[[74,1.5],[76,.5],[74,1],[71,1]],
 [[76,1],[79,1],[84,1],[83,.5],[81,.5]],[[79,1.5],[76,.5],[71,1],[76,1]],[[81,1],[79,.5],[77,.5],[76,1],[72,1]],[[74,1],[67,.5],[69,.5],[71,1],[74,1]]];
function mNote(f,t,dur,type,vol,dest,filt){const o=AC.createOscillator(),g=AC.createGain();o.type=type;o.frequency.setValueAtTime(f,t);g.gain.setValueAtTime(0,t);g.gain.linearRampToValueAtTime(vol,t+.01);g.gain.setTargetAtTime(0,t+dur*.6,dur*.25+.02);
 let n=o;if(filt){const fl=AC.createBiquadFilter();fl.type='lowpass';fl.frequency.value=filt;o.connect(fl);n=fl}n.connect(g).connect(dest);o.start(t);o.stop(t+dur+.5)}
function nbuf(){if(!NB){const n=AC.sampleRate;NB=AC.createBuffer(1,n,n);const d=NB.getChannelData(0);for(let i=0;i<n;i++)d[i]=Math.random()*2-1}return NB}
function mNoise(t,dur,freq,type,vol,dest){const s=AC.createBufferSource();s.buffer=nbuf();const f=AC.createBiquadFilter();f.type=type;f.frequency.value=freq;const g=AC.createGain();g.gain.setValueAtTime(vol,t);g.gain.exponentialRampToValueAtTime(.0001,t+dur);s.connect(f).connect(g).connect(dest);s.start(t,Math.random()*.4);s.stop(t+dur+.02)}
function mKick(t,dest){const o=AC.createOscillator(),g=AC.createGain();o.frequency.setValueAtTime(140,t);o.frequency.exponentialRampToValueAtTime(42,t+.18);g.gain.setValueAtTime(.55,t);g.gain.exponentialRampToValueAtTime(.0001,t+.3);o.connect(g).connect(dest);o.start(t);o.stop(t+.32)}
function scheduleBar(i,t,dest){const b=i%8,C=CH[b],B=BEAT;
 if(b===0)mNoise(t,1.4,6000,'highpass',.12,dest);
 for(let k=0;k<8;k++)mNoise(t+k*B/2,.05,9000,'highpass',k%2?.05:.035,dest);
 mKick(t,dest);mKick(t+2*B,dest);if(b%2)mKick(t+2.5*B,dest);
 [1,3].forEach(k=>{mNoise(t+k*B,.16,1900,'bandpass',.22,dest);mNote(190,t+k*B,.08,'triangle',.12,dest)});
 if(b===7)for(let k=0;k<4;k++)mNoise(t+(3+k/4)*B,.08,2200,'bandpass',.12+k*.04,dest);
 [[0,0,.8],[1,12,.4],[1.5,0,.4],[2,7,.8],[3,0,.4],[3.5,12,.4]].forEach(([at,iv,d])=>mNote(mf(C.r+iv),t+at*B,d*B,'triangle',.28,dest));
 [.5,1.5,2.5,3.5].forEach(at=>C.t.forEach(n=>mNote(mf(n),t+at*B,.22*B,'square',.022,dest,1800)));
 let at=0;for(const[n,d]of MEL[b]){mNote(mf(n),t+at*B,d*B*.95,'triangle',.16,dest);mNote(mf(n),t+at*B,d*B*.9,'square',.035,dest,2600);at+=d}}
function musicStart(){if(MUS||!ac())return;const bus=AC.createGain(),t=AC.currentTime;bus.gain.setValueAtTime(0,t);bus.gain.linearRampToValueAtTime(.55,t+1);bus.connect(AC.destination);
 const m={bus,bar:0,next:t+.15};const pump=()=>{while(m.next<AC.currentTime+.7){scheduleBar(m.bar,m.next,bus);m.bar++;m.next+=BEAT*4}};pump();m.iv=setInterval(pump,120);MUS=m}
function musicStop(){if(!MUS)return;const m=MUS;MUS=null;clearInterval(m.iv);const t=AC.currentTime;m.bus.gain.cancelScheduledValues(t);m.bus.gain.setValueAtTime(m.bus.gain.value,t);m.bus.gain.linearRampToValueAtTime(0,t+.5);setTimeout(()=>{try{m.bus.disconnect()}catch(e){}},3000)}
function musicSync(){const lobby=$('game').hidden;if(musicOn&&unlocked&&lobby&&!document.hidden)musicStart();else musicStop()}
function unlockAudio(){if(unlocked)return;unlocked=true;ac();musicSync()}
document.addEventListener('pointerdown',unlockAudio);document.addEventListener('keydown',unlockAudio);
document.addEventListener('visibilitychange',musicSync);

/* ================= FX: particles, callouts, popups ================= */
const fx=$('fx'),fctx=fx.getContext('2d');let parts=[],fxRun=false,DPR=1;
function fxSize(){DPR=Math.min(2,window.devicePixelRatio||1);fx.width=Math.round(innerWidth*DPR);fx.height=Math.round(innerHeight*DPR)}
addEventListener('resize',fxSize);fxSize();
function burst(x,y,colors,n=16,o={}){if(reduced)return;for(let i=0;i<n;i++){const a=Math.random()*Math.PI*2,sp=(o.sp||5)*(.35+Math.random());
 parts.push({x,y,vx:Math.cos(a)*sp,vy:Math.sin(a)*sp-(o.up??2),g:o.g??.28,life:1,dec:.016+Math.random()*.02,s:(o.s||6)*(.6+Math.random()*.8),c:colors[i%colors.length],r:Math.random()*6,vr:(Math.random()-.5)*.4,sh:o.sh||(i%3?'sq':'st')})}
 if(!fxRun){fxRun=true;requestAnimationFrame(fxLoop)}}
function confetti(){if(reduced)return;const cs=[...COL.map(c=>c.cc),'#ffd23a','#bb73ff'];for(let i=0;i<140;i++)parts.push({x:Math.random()*innerWidth,y:-20-Math.random()*innerHeight*.5,vx:(Math.random()-.5)*2,vy:2+Math.random()*3,g:.04,life:1,dec:.0035+Math.random()*.003,s:6+Math.random()*5,c:cs[i%cs.length],r:Math.random()*6,vr:(Math.random()-.5)*.3,sh:'cf'});
 if(!fxRun){fxRun=true;requestAnimationFrame(fxLoop)}}
function star(c,r){c.beginPath();for(let i=0;i<10;i++){const a=i*Math.PI/5-Math.PI/2,rr=i%2?r*.45:r;c.lineTo(Math.cos(a)*rr,Math.sin(a)*rr)}c.closePath();c.fill()}
function fxLoop(){fctx.setTransform(DPR,0,0,DPR,0,0);fctx.clearRect(0,0,innerWidth,innerHeight);parts=parts.filter(p=>p.life>0&&p.y<innerHeight+40);
 for(const p of parts){p.x+=p.vx;p.y+=p.vy;p.vy+=p.g;p.vx*=.985;p.r+=p.vr;p.life-=p.dec;fctx.globalAlpha=Math.max(0,Math.min(1,p.life*1.6));fctx.fillStyle=p.c;fctx.save();fctx.translate(p.x,p.y);fctx.rotate(p.r);
  if(p.sh==='st')star(fctx,p.s*.8);else if(p.sh==='cf')fctx.fillRect(-p.s/2,-p.s*.8,p.s,p.s*1.6);else{fctx.fillRect(-p.s/2,-p.s/2,p.s,p.s);fctx.strokeStyle='rgba(11,22,64,.6)';fctx.lineWidth=1;fctx.strokeRect(-p.s/2,-p.s/2,p.s,p.s)}fctx.restore()}
 fctx.globalAlpha=1;if(parts.length)requestAnimationFrame(fxLoop);else{fxRun=false;fctx.clearRect(0,0,innerWidth,innerHeight)}}
const centerOf=el=>{const r=el.getBoundingClientRect();return[r.left+r.width/2,r.top+r.height/2]};
function shake(n=1){if(reduced)return;const g=$('game');g.classList.remove('shk','shk2');void g.offsetWidth;g.classList.add(n>1?'shk2':'shk')}
let coQ=Promise.resolve();
function callout(text,c1='#ffd23a',sub=''){coQ=coQ.then(()=>new Promise(res=>{const d=document.createElement('div');d.className='callout';
 d.innerHTML=`<div class="co-t ol" style="--c1:${c1};color:${c1}">${esc(text)}</div>${sub?`<div class="co-s ol">${esc(sub)}</div>`:''}`;$('callouts').appendChild(d);
 setTimeout(()=>d.remove(),1300);setTimeout(res,reduced?250:720)}))}
function popup(el,text,cls){if(!el)return;const[x,y]=centerOf(el);const d=document.createElement('div');d.className='popn ol '+cls;d.textContent=text;d.style.left=x+'px';d.style.top=y+'px';document.body.appendChild(d);setTimeout(()=>d.remove(),1300)}
/* ================= Shop: table themes (your screen) and card backs (what rivals see) ================= */
const SHOP0=[['classic','table',0],['peach','table',0],['slate','table',0],['candy','table',300],['bubble','table',300],['ocean','table',300],['blossom','table',450],['camo','table',450],['neon','table',450],['steel','table',600],['volcano','table',600],
 ['stripes','back',0],['dots','back',0],['chevron','back',0],['paws','back',200],['stars','back',250],['flames','back',300],['bolt','back',300],['circuit','back',400],['crest','back',750],['summit','back',0,1400],['laurel','back',0,0,1]].map(([id,kind,price,tr,cup])=>({id,kind,price,tr:tr||0,cup:!!cup}));
const SHOPN={classic:'Classic Blue',peach:'Peach Sorbet',slate:'Slate Night',candy:'Candy Pop',bubble:'Bubble Tea',ocean:'Ocean Breeze',blossom:'Cherry Blossom',camo:'Jungle Camo',neon:'Neon Arcade',steel:'Midnight Steel',volcano:'Volcanic',
 stripes:'Classic Stripes',dots:'Polka Dots',chevron:'Chevron',paws:'Paw Prints',stars:'Stars & Moon',flames:'Flames',bolt:'Lightning Bolt',circuit:'Circuit Board',crest:'Golden Crest',summit:'Summit Crown',laurel:'Champion\u2019s Laurel'};
const AI_BACKS=['stripes','dots','chevron','paws','flames','circuit','crest'];
const okItem=(id,kind)=>SHOP0.some(i=>i.id===id&&i.kind===kind);
let EQ={table:'classic',back:'stripes'},shopTab='table',shopArmed=null;
function shopItems(){if(ME&&ME.shop)return ME.shop.items;const t=Math.max(REC.best,REC.tr);return SHOP0.map(i=>({...i,have:i.cup?false:i.tr?t>=i.tr:i.price===0}))}
function applyTheme(){const b=document.body;[...b.classList].filter(c=>c.startsWith('tb-')).forEach(c=>b.classList.remove(c));b.classList.add('tb-'+EQ.table)}
function setBk(el,id){if(!el)return;[...el.classList].filter(c=>c.startsWith('bk-')).forEach(c=>el.classList.remove(c));if(id&&id!=='stripes')el.classList.add('bk-'+id)}
function oppBack(){if(M&&M.mode==='ai')return AI_BACKS[Math.min(AI_BACKS.length-1,M.tier||0)];return M&&M.opp&&okItem(M.opp.back,'back')?M.opp.back:'stripes'}
function equipLocal(kind,id){EQ[kind]=id;ST.set('ft-'+kind,id);applyTheme()}
function renderShop(){const body=$('shopBody');if(!body)return;if(shopTab==='coins'&&!iapReady())shopTab='table';const items=shopItems().filter(i=>i.kind===shopTab),coins=ME?ME.coins:null;
 const sw=id=>shopTab==='table'?`<div class="tsw tb-${id}">${[0,1,2,3,4].map(c=>`<i style="left:${20+c*13}%;background:${COL[c].cc}"></i>`).join('')}</div>`:`<div class="back bkp bk-${id}"></div>`;
 const chip=i=>{if(EQ[i.kind]===i.id)return'<span class="pr own">In use</span>';if(i.have)return'<span class="pr use">Use</span>';
  if(i.cup)return`<span class="pr lock">Win a cup</span>`;
  if(i.tr)return`<span class="pr lock">${IC.cup}${i.tr}</span>`;if(shopArmed===i.id)return`<span class="pr buy">Buy? ${COIN}${i.price}</span>`;
  return`<span class="pr buy ${coins!=null&&coins<i.price?'poor':''}">${COIN}${i.price}</span>`};
 body.innerHTML=`<div class="shoptabs" role="tablist"><button type="button" data-st="table" class="${shopTab==='table'?'on':''}">Tables</button><button type="button" data-st="back" class="${shopTab==='back'?'on':''}">Card backs</button>${iapReady()?`<button type="button" data-st="coins" class="${shopTab==='coins'?'on':''}">Get coins</button>`:''}</div>
  ${coins!=null?`<div class="shopbal">${COIN}<span>${coins}</span></div>`:''}
  ${shopTab==='coins'?iapGridHTML():`<div class="shopgrid">${items.map(i=>`<button type="button" class="si ${EQ[i.kind]===i.id?'eq':''} ${shopArmed===i.id?'armed':''}" data-id="${i.id}" aria-label="${SHOPN[i.id]||i.id}">${sw(i.id)}<span class="nm">${SHOPN[i.id]||i.id}</span>${chip(i)}</button>`).join('')}</div>`}
  <p class="shopnote">${shopTab==='coins'?'Paid securely through the App Store. Coins land in your piggy bank as soon as Apple confirms.':shopTab==='table'?'Your table theme changes how the game looks for you.':'Your rival sees your card back during battles.'}${ME?'':' Go online to buy with coins.'}</p>`;
 body.querySelectorAll('[data-st]').forEach(b=>b.onclick=()=>{sfx('ui');shopTab=b.dataset.st;shopArmed=null;renderShop()});
 body.querySelectorAll('.si').forEach(b=>b.onclick=()=>shopTap(b.dataset.id));
 body.querySelectorAll('[data-pack]').forEach(b=>b.onclick=()=>iapBuy(b.dataset.pack,b))}
/* ================= Coin packs: App Store purchases inside the iPhone app =================
   Uses the free cordova-plugin-purchase (window.CdvPurchase), which only exists in the iPhone
   app. The server checks every purchase with Apple before adding coins, and replays of a
   purchase it already paid are answered at once so the app can close them. */
let IAP_PACKS=null,iapStarted=false;const IAP_PENDING={};
const IAP_EXTRA={coins_1000:'+11% bonus',coins_1800:'+20% bonus',coins_4000:'+33% · best value'};
const iapStore=()=>window.CdvPurchase&&window.CdvPurchase.store||null;
function iapReady(){return !!(IAP_PACKS&&IAP_PACKS.length&&iapStore())}
function iapStart(){const st=iapStore();if(iapStarted||!st||!IAP_PACKS)return;iapStarted=true;const C=window.CdvPurchase;
 try{st.register(IAP_PACKS.map(p=>({id:p.id,type:C.ProductType.CONSUMABLE,platform:C.Platform.APPLE_APPSTORE})));
  st.when().productUpdated(()=>{if(!$('shopSheet').hidden&&shopTab==='coins')renderShop()})
   .approved(tx=>{if(!tx||!tx.transactionId)return;IAP_PENDING[tx.transactionId]=tx;iapSend(tx.transactionId)});
  st.error(e=>{if(e&&e.code!==C.ErrorCode.PAYMENT_CANCELLED)console.warn('store',e&&e.message)});
  st.initialize([C.Platform.APPLE_APPSTORE]).catch(()=>{})}catch(e){console.warn('purchases unavailable',e)}}
function iapSend(tx){if(NET.ready)NET.send({t:'iap_verify',tx});}
function iapResend(){Object.keys(IAP_PENDING).forEach(iapSend)}
function iapPrice(id){try{const C=window.CdvPurchase,p=iapStore().get(id,C.Platform.APPLE_APPSTORE),pr=p&&p.pricing;return pr&&pr.price?esc(pr.price):'…'}catch(e){return'…'}}
function iapGridHTML(){return`<div class="shopgrid packs">${IAP_PACKS.map((p,i)=>`<button type="button" class="si pack" data-pack="${p.id}" aria-label="${p.coins} coins">
  <div class="packart" style="--n:${i}">${Array.from({length:Math.min(5,i+2)},(_,k)=>`<span style="--k:${k}">${COIN}</span>`).join('')}</div>
  <span class="nm"><b>${p.coins.toLocaleString()}</b> coins</span>${IAP_EXTRA[p.id]?`<small class="bonus">${IAP_EXTRA[p.id]}</small>`:''}<span class="pr buy">${iapPrice(p.id)}</span></button>`).join('')}</div>`}
async function iapBuy(id,el){const C=window.CdvPurchase,st=iapStore();if(!st)return;
 if(!NET.ready){sfx('bad');toast('Connect to the game server first, so your coins can be added.');return}
 const p=st.get(id,C.Platform.APPLE_APPSTORE),offer=p&&p.getOffer();
 if(!offer){sfx('bad');toast('That pack is not available from the App Store right now.');return}
 sfx('ui');iapFrom=el;
 try{const err=await offer.order();if(err&&err.code!==C.ErrorCode.PAYMENT_CANCELLED){sfx('bad');toast(err.message||'The purchase did not go through.')}}catch(e){sfx('bad');toast('The purchase did not go through.')}}
let iapFrom=null;
function onIapOk(m){const tx=String(m.tx||''),t=IAP_PENDING[tx];if(t){try{t.finish()}catch(e){}delete IAP_PENDING[tx]}
 const added=numOf(m.added);if(added>0){sfx('levelup');callout(`+${added.toLocaleString()} coins`,'#ffd23a','Thank you!');depositCoins(added,iapFrom,Math.max(0,numOf(m.coins)-added));iapFrom=null}}
const IAP_TRIES={};
function onIapFail(m){const tx=String(m.tx||''),code=String(m.code||'');
 // Apple may need a moment to know a brand-new purchase: keep it open and ask again a few times.
 if(code==='iap_not_found'&&IAP_PENDING[tx]&&(IAP_TRIES[tx]=(IAP_TRIES[tx]||0)+1)<=5){toast('Apple has not confirmed the purchase yet. Your coins will be added shortly.');setTimeout(()=>{if(IAP_PENDING[tx])iapSend(tx)},20000*IAP_TRIES[tx]);return}
 if(code==='iap_retry'){toast('Apple has not confirmed the purchase yet. Your coins will be added shortly.');setTimeout(()=>{if(IAP_PENDING[tx])iapSend(tx)},30000);return}
 const t=IAP_PENDING[tx];if(t&&code!=='iap_off'){try{t.finish()}catch(e){}delete IAP_PENDING[tx]}
 sfx('bad');toast(code==='iap_other_account'?'That purchase belongs to another account.':code==='iap_revoked'?'That purchase was refunded.':code==='iap_off'?'Purchases are not set up on this server yet.':'That purchase could not be confirmed. Contact support if you were charged.')}
function shopTap(id){const i=shopItems().find(x=>x.id===id);if(!i)return;
 if(EQ[i.kind]===id){sfx('ui');return}
 if(i.have){sfx('select');shopArmed=null;equipLocal(i.kind,id);if(ME&&NET.ready)NET.send({t:'equip',kind:i.kind,item:id});renderShop();return}
 if(i.cup){sfx('bad');toast(`Win a cup to unlock ${SHOPN[id]}.`);return}
 if(i.tr){sfx('bad');toast(`Reach ${i.tr} trophies to unlock ${SHOPN[id]}.`);return}
 if(!ME||!NET.ready){sfx('bad');toast('Go online to buy with coins.');return}
 if(ME.coins<i.price){sfx('bad');toast(`You need ${i.price-ME.coins} more coins for ${SHOPN[id]}.`);return}
 if(shopArmed!==id){sfx('select');shopArmed=id;renderShop();return}
 shopArmed=null;NET.send({t:'buy',item:id});renderShop()}
function onBought(m){const id=String(m.item||''),it=SHOP0.find(i=>i.id===id);if(!it)return;sfx('coin');setTimeout(()=>sfx('levelup'),120);
 equipLocal(it.kind,id);NET.send({t:'equip',kind:it.kind,item:id});
 const el=document.querySelector(`.si[data-id="${id}"]`);if(el&&el.getBoundingClientRect().width){const[x,y]=centerOf(el);burst(x,y,['#ffd23a','#fff','#7dffa0'],26,{sp:7})}
 callout(SHOPN[id],'#ffd23a','Yours! Equipped')}
EQ={table:okItem(ST.get('ft-table','classic'),'table')?ST.get('ft-table','classic'):'classic',back:okItem(ST.get('ft-back','stripes'),'back')?ST.get('ft-back','stripes'):'stripes'};
function toast(t){document.querySelectorAll('.toast').forEach(e=>e.remove());const d=document.createElement('div');d.className='toast';d.textContent=t;document.body.appendChild(d);setTimeout(()=>d.remove(),2800)}
function tween(ms,fn,done){if(reduced){fn(1);done&&done();return}const t0=performance.now();const step=t=>{const k=Math.min(1,(t-t0)/ms);fn(1-Math.pow(1-k,3));if(k<1)requestAnimationFrame(step);else done&&done()};requestAnimationFrame(step)}

/* ================= Player data (IndexedDB, this browser only) ================= */
const REC0=()=>({ai:{w:0,l:0,d:0},pvp:{w:0,l:0,d:0},opp:{},recent:[],tr:0,best:0,xp:0});
function normRec(r){const o=Object.assign(REC0(),r&&typeof r==='object'?r:{});if(r&&r.tr==null){o.tr=Math.max(0,o.ai.w*12-o.ai.l*8+o.pvp.w*30-o.pvp.l*20);o.best=o.tr;o.xp=(o.ai.w+o.ai.l+o.ai.d+o.pvp.w+o.pvp.l+o.pvp.d)*45}return o}
let REC=normRec(ST.get('ft-record',null));
const NAME_A=['Brave','Swift','Bold','Calm','Lucky','Sunny','Misty','Clever','Daring','Nimble','Steady','Wild'];
const NAME_N=['Fox','Owl','Yak','Lynx','Hawk','Otter','Ibex','Wolf','Crane','Bison','Tiger','Panda'];
/* A default name for a new player, e.g. SwiftOtter482. The server keeps names unique and adds digits if it is taken. */
function genName(){const p=a=>a[Math.floor(Math.random()*a.length)];return p(NAME_A)+p(NAME_N)+(100+Math.floor(Math.random()*900))}
function cleanNick(raw){return String(raw??'').replace(/[^\p{L}\p{N} \-_.']/gu,'').replace(/\s+/g,' ').trim().slice(0,16)}
let NAME=cleanNick(ST.get('ft-nick',''));
if(!NAME){NAME=genName();ST.set('ft-nick',NAME)}
/* Card styles: one per arena, unlocked by reaching that arena's trophy count. */
const CARDSETS=[{k:'classic',n:'Classic'},{k:'sandstone',n:'Sandstone'},{k:'pearl',n:'Pearl'},{k:'jade',n:'Jade'},{k:'ember',n:'Ember'},{k:'frost',n:'Frost'},{k:'legend',n:'Legend'}];
let cardSel=Math.max(0,Math.min(CARDSETS.length-1,Math.floor(Number(ST.get('ft-cards',0)))||0));
const cardsOpen=()=>arenaIdx(Math.max(REC.best,REC.tr));
function applyCards(){if(cardSel>cardsOpen())cardSel=0;const b=document.body;CARDSETS.forEach(s=>b.classList.remove('cs-'+s.k));b.classList.add('cs-'+CARDSETS[cardSel].k)}
let PID=ST.get('pid',null);
if(typeof PID!=='string'||!/^[A-Za-z0-9-]{8,64}$/.test(PID)){PID=crypto.randomUUID?crypto.randomUUID():Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');ST.set('pid',PID)}
/* Secret account key: 20 symbols (100 bits) with no 0/O/1/I. It never goes to anyone but the game server, which stores only its hash. */
const KEY_ALPHA='ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
const genKey=()=>Array.from(crypto.getRandomValues(new Uint8Array(20)),b=>KEY_ALPHA[b&31]).join('');
function cleanKey(raw){const k=String(raw??'').replace(/[\s-]/g,'').toUpperCase();return k.length===20&&[...k].every(c=>KEY_ALPHA.includes(c))?k:null}
const fmtKey=k=>k.match(/.{5}/g).join('-');
let KEY=cleanKey(ST.get('ft-key',''));
if(!KEY){KEY=genKey();ST.set('ft-key',KEY)}
/* While restoring another account the old key is remembered here (also across a reload) until the server answers. */
const RESTORE={prev:cleanKey(ST.get('ft-restore',''))};
const nick=()=>(NAME||'Explorer').slice(0,16);
function saveRec(){ST.set('ft-record',REC)}
const RESULT={};
function recordGame(mode,oppName,oppKey,me,them,gid,r,noTrophies){
 if(RESULT[gid])return RESULT[gid];
 const t0=REC.tr,x0=REC.xp,a0=arenaIdx(t0);
 if(REC.recent.some(x=>x.g===gid))return RESULT[gid]={r,t0,t1:t0,x0,x1:x0,a0,a1:a0,dT:0,dX:0};
 const b=mode==='ai'?REC.ai:REC.pvp;b[r]++;
 if(mode==='pvp'){const o=REC.opp[oppKey]||{name:oppName,w:0,l:0,d:0};o.name=oppName;o[r]++;REC.opp[oppKey]=o}
 const D=noTrophies?{w:0,l:0,d:0}:mode==='ai'?{w:12,l:-8,d:0}:{w:30,l:-20,d:0};
 const t1=Math.max(ARENAS[a0].at,t0+D[r]);
 const dX=20+(r==='w'?30:r==='d'?15:5)+Math.floor(Math.max(0,Math.min(me,120))/4);
 REC.tr=t1;REC.best=Math.max(REC.best,t1);REC.xp=x0+dX;
 if(arenaIdx(t1)>a0){cardSel=arenaIdx(t1);ST.set('ft-cards',cardSel);applyCards()}
 REC.recent.unshift({m:mode,o:oppName,me,them,r,t:Date.now(),g:gid,dt:t1-t0});REC.recent=REC.recent.slice(0,12);saveRec();syncProg();
 return RESULT[gid]={r,t0,t1,x0,x1:x0+dX,a0,a1:arenaIdx(t1),dT:t1-t0,dX}}
/* ---- Backup of lobby progress on the server (unverified; never used for rewards or the leaderboard) ---- */
const progOf=()=>({tr:REC.tr,best:REC.best,xp:REC.xp,ai:[REC.ai.w,REC.ai.l,REC.ai.d],pvp:[REC.pvp.w,REC.pvp.l,REC.pvp.d]});
function syncProg(){if(NET.ready)NET.raw(Object.assign({t:'sync'},progOf()))}
function validProg(p){const n=v=>Number.isFinite(v)&&v>=0;return !!p&&typeof p==='object'&&n(p.tr)&&n(p.best)&&n(p.xp)&&[p.ai,p.pvp].every(a=>Array.isArray(a)&&a.length===3&&a.every(n))}
function adoptProg(p){const x=p||{tr:0,best:0,xp:0,ai:[0,0,0],pvp:[0,0,0]};
 REC=normRec(Object.assign(REC0(),{tr:Math.floor(x.tr),best:Math.floor(Math.max(x.best,x.tr)),xp:Math.floor(x.xp),ai:{w:x.ai[0],l:x.ai[1],d:x.ai[2]},pvp:{w:x.pvp[0],l:x.pvp[1],d:x.pvp[2]}}));
 saveRec();if(cardSel>cardsOpen()){cardSel=0;ST.set('ft-cards',0)}applyCards();renderHome()}
const tot=()=>({w:REC.ai.w+REC.pvp.w,l:REC.ai.l+REC.pvp.l,d:REC.ai.d+REC.pvp.d});
const wld=o=>`${o.w}W · ${o.l}L${o.d?` · ${o.d}D`:''}`;

/* ================= Network: stateless match server ================= */
const SERVER_URL=(()=>{const m=document.querySelector('meta[name="five-trails-server"]');const u=m&&m.content.trim();if(u)return u;
 if(location.protocol!=='http:'&&location.protocol!=='https:')return null;
 const proto=location.protocol==='https:'?'wss://':'ws://';
 const basePath=(location.pathname||'/').replace(/\/[^/]*$/, '/');
 const wsPath=(basePath+'ws').replace(/\/+/g, '/');
 return proto+location.host+wsPath})();
const NET={ws:null,ready:false,stats:null,backoff:800,pingT:null,
 connect(){if(!SERVER_URL||(this.ws&&this.ws.readyState<2))return;let ws;try{ws=new WebSocket(SERVER_URL)}catch(e){netChanged();return}
  this.ws=ws;
  ws.onopen=()=>{this.backoff=800;const h={t:'hello',v:2,name:safeName(),pid:PID,key:KEY,tr:REC.tr,lv:lvlInfo(REC.xp).lv};if(RESTORE.prev)h.restore=true;const tok=SS.get('ft-resume');if(tok)h.resume=tok;this.raw(h);
   clearInterval(this.pingT);this.pingT=setInterval(()=>this.raw({t:'ping'}),25000)};
  ws.onmessage=e=>{if(typeof e.data!=='string'||e.data.length>65536)return;let m;try{m=JSON.parse(e.data)}catch(_){return}if(m&&typeof m.t==='string')onServer(m)};
  ws.onclose=()=>{clearInterval(this.pingT);const was=this.ready;this.ws=null;this.ready=false;netDown(was);netChanged();
   setTimeout(()=>this.connect(),this.backoff+Math.random()*400);this.backoff=Math.min(15000,this.backoff*2)};
  ws.onerror=()=>{}},
 reconnect(){this.backoff=100;if(this.ws){try{this.ws.close()}catch(e){}}else this.connect()},
 raw(m){if(this.ws&&this.ws.readyState===1){try{this.ws.send(JSON.stringify(m))}catch(e){}}},
 send(m){if(!this.ready){toast(SERVER_URL?'Still connecting to the game server. Try again in a moment.':'Online play needs the game server.');return false}this.raw(m);return true}};
function sendProfile(){NET.raw({t:'profile',name:safeName(),tr:REC.tr,lv:lvlInfo(REC.xp).lv})}
function safeName(){return cleanNick(nick())||'Explorer'}
/* Names are unique. The server confirms a change (`name`) or refuses it (`name_taken`). */
let namePending=null;
function commitName(n){NAME=n;ST.set('ft-nick',n);renderTop();const i=$('nick');if(i)i.value=n}
function sendName(n,done){if(namePending)clearTimeout(namePending.t);
 namePending={done,t:setTimeout(()=>{if(namePending){const p=namePending;namePending=null;p.done(false,'Could not reach the server. Try again.')}},5000)};
 NET.raw({t:'profile',name:n,tr:REC.tr,lv:lvlInfo(REC.xp).lv})}
function trySetName(raw,done){
 const n=cleanNick(raw);
 if(!n){done(false,'Use 1 to 16 letters, numbers or spaces.');return}
 if(n===NAME&&!ST.get('ft-nick-pending',false)){done(true);return}
 if(NET.ready)sendName(n,done);
 else{commitName(n);ST.set('ft-nick-pending',true);done(true,SERVER_URL?'Saved. The server will check it when you go online.':'')}}
function nameReply(ok,msg){if(!namePending)return false;const p=namePending;namePending=null;clearTimeout(p.t);p.done(ok,msg);return true}
const ERRS={room_not_found:'No open room has that code. Check it and try again.',too_many_attempts:'Too many wrong codes. Wait a minute and try again.',busy:'Finish what you are doing first.',
 server_full:'The server is full right now. Try again soon.',bad_hello:'The server did not accept your name. Try a simpler one in Settings.',version:'This page is out of date. Reload to play online.',
 opponent_left:'Your opponent has left.',opponent_away:'Waiting for your opponent to reconnect.',rate_limited:'Slow down a little.',replaced:'This battle continued in another tab.',
 code_taken:'That room code is already in use. Pick another one.',bad_code:'A room code is exactly 5 letters.',name_taken:'That name is taken. Try another.',
 no_chest:'No chest is ready yet.',quest_not_done:'Finish that quest first.',already_claimed:'Already collected.',bad_quest:'That quest is not available any more.',
 bot_unavailable:'The computer is not available yet. Keep waiting a little.',not_enough_coins:'Not enough coins yet.',cup_not_found:'No cup has that code. Check it and try again.',cup_full:'That cup is full (6 players).',cup_started:'That cup has already started.',cup_not_host:'Only the host can start the cup.',cup_too_few:'A cup needs at least 3 players to start.',cup_not_your_match:'That is not your match.',cup_match_closed:'That match is already decided.',cup_too_many:'You are in 5 cups already. Finish one first.',cup_bad_name:'Give the cup a name (up to 24 letters).',cup_no_rematch:'Cup matches are one game each. Head back to the cup.',bad_format:'Pick round robin or knockout.',not_live:'That match is not being played right now.',watch_full:'Too many people are watching that match.',already_owned:'You already own that.',not_for_sale:'That one is not sold.',item_locked:'Win more trophies to unlock that.',bad_item:'That item is not available.',unknown_key:'No account has that recovery code.'};
function onServer(m){switch(m.t){
 case 'welcome':onWelcome(m);break;
 case 'name':if(typeof m.name==='string'&&m.name){commitName(m.name);ST.set('ft-nick-pending',false);nameReply(true)}break;
 case 'me':ME=cleanMe(m);if(ME.shop){EQ.table=ME.shop.table;EQ.back=ME.shop.back;ST.set('ft-table',EQ.table);ST.set('ft-back',EQ.back);applyTheme()}if(!$('shopSheet').hidden)renderShop();renderRewards();if(!$('questSheet').hidden)renderQuests();scheduleReminders();maybeShowReward();break;
 case 'claimed':onClaimed(m);break;
 case 'bought':onBought(m);break;
 case 'iap_ok':onIapOk(m);break;
 case 'iap_fail':onIapFail(m);break;
 case 'cup':onCup(m.cup);break;
 case 'cups':if(Array.isArray(m.cups)){const keep={};m.cups.forEach(v=>{const c=cleanCup(v);if(c)keep[c.code]=c});CUPS=keep;if(curTab==='cups')renderCups();paintCupDot();if(cupOpen&&!$('cupSheet').hidden)renderCupSheet()}break;
 case 'cup_gone':{const code=String(m.code||'');delete CUPS[code];if(cupOpen===code&&!$('cupSheet').hidden){closeSheets();toast('That cup was closed.')}if(curTab==='cups')renderCups();paintCupDot();break}
 case 'cup_wait':M={mode:'cupwait',code:String(m.code||''),m:numOf(m.m,99)};closeSheets();showWait('Cup match','',`Waiting for ${String(m.opp||'your opponent').slice(0,16)} to tap Play. We have told them you are here.`);break;
 case 'cup_unwait':if(M.mode==='cupwait')goHome(true);break;
 case 'cup_ping':{const code=String(m.code||''),i=numOf(m.m,99);cupPings[code+':'+i]=Date.now();const c=CUPS[code];sfx('join');
  toast(`${String(m.from||'Your opponent').slice(0,16)} is waiting for your cup match${c?' in '+c.name:''}. Open Cups to play.`);renderCupBits(code);break}
 case 'cup_prize':{const place=numOf(m.place,9),coins=numOf(m.coins);sfx(place===1?'win':'levelup');if(place===1)confetti();
  callout(place===1?'Cup champion!':place===2?'Runner-up!':'Cup complete','#ffd23a',String(m.name||'').slice(0,24));
  setTimeout(()=>depositCoins(coins,null,ME?ME.coins:0),900);break}
 case 'earned':if(M.mode==='online'){M.earned={coins:numOf(m.coins),counted:m.counted!==false,cup:typeof m.cup==='string'};if(!$('over').hidden)showEarned(true)}break;
 case 'bot_offer':if(M.mode==='queue'){$('botOffer').hidden=false;sfx('join')}break;
 case 'leaderboard':{const l=cleanLB(m);LBS[l.scope]=l;if(curTab==='leaders')renderLeaders();break}
 case 'stats':NET.stats=m.stats||null;netChanged();break;
 case 'queued':M={mode:'queue'};showWait('Finding an opponent','','Looking for a player near your trophies…');break;
 case 'unqueued':case 'room_closed':if(M.mode==='queue'||M.mode==='room')goHome(true);break;
 case 'room':M={mode:'room',code:String(m.code||'').replace(/[^A-Z]/g,''),expires:Date.now()+(Math.max(10,Number(m.ttl)||120))*1000};showWait('Room code',M.code,'Send this code to a friend. Waiting for them…');break;
 case 'room_expired':if(M.mode==='room'){goHome(true);toast('Nobody joined in time, so the room closed. Create a new one any time.')}break;
 case 'match':onMatch(m);break;
 case 'state':if(M.mode==='online'&&m.view&&typeof m.view==='object'){V=m.view;V.at=Date.now();pending=false;M.reconnecting=false;renderGame();updateRmNote()}break;
 case 'emote':if(M.mode==='online'&&Number.isInteger(m.k)&&EMOTES[m.k])showBubble('opp',EMOTES[m.k]);else if(M.mode==='watch'&&Number.isInteger(m.k)&&EMOTES[m.k])showBubble(m.side===0?'me':'opp',EMOTES[m.k]);break;
 case 'watch_start':{const v=m.view;if(!v||typeof v!=='object'||!Array.isArray(m.players))break;
  const pl=m.players.slice(0,2).map(p=>({n:String(p&&p.name||'Player').slice(0,16),lv:Math.max(1,Number(p&&p.lv)||1),tr:numOf(p&&p.tr),back:okItem(p&&p.back,'back')?p.back:'stripes'}));
  closeSheets();M={mode:'watch',players:pl,opp:{back:pl[1].back},oppHere:true,kind:'cup',code:typeof m.code==='string'?m.code:null,cupM:numOf(m.cupMatch,99)};
  V=v;V.at=Date.now();prevGid=null;sel=null;pending=false;$('over').hidden=true;
  const c=CUPS[M.code];$('roomTag').textContent=`Watching${c?' · '+c.name:''}`;show('game');renderGame();break}
 case 'watch_state':if(M.mode==='watch'&&m.view&&typeof m.view==='object'){V=m.view;V.at=Date.now();renderGame()}break;
 case 'watch_end':if(M.mode==='watch'){if(V&&!V.ov){toast('The match has ended.');stopWatching(true)}}break;
 case 'watchers':M.watchers=numOf(m.n,99);paintWatchers();break;
 case 'opp':if(M.mode==='online'){M.oppHere=!!m.connected;M.oppLeft=!!m.left;if(V)renderGame();updateRmNote()}break;
 case 'rematch':if(M.mode==='online'&&V){M.iWantRm=m.me?V.gid:null;M.theyWantRm=m.opp?V.gid:null;updateRmNote()}break;
 case 'closed':if(M.mode==='online'){M.closed=true;SS.set('ft-resume',null);updateRmNote()}break;
 case 'resume_failed':SS.set('ft-resume',null);if(M.mode==='online'&&!(V&&V.ov)){goHome(true);toast('That battle has ended.')}break;
 case 'error':
  if(m.code==='unknown_key'&&RESTORE.prev){KEY=RESTORE.prev;ST.set('ft-key',KEY);RESTORE.prev=null;ST.set('ft-restore','');keyNote(ERRS.unknown_key+' Check it and try again.',true);paintKey();break}
  if((m.code==='name_taken'||m.code==='bad_hello')&&nameReply(false,m.code==='name_taken'?ERRS.name_taken:'Use letters, numbers or spaces.'))break;
  pending=false;if(V&&M.mode==='online')renderActs();if(m.code!=='stale'&&m.code!=='rate_limited')toast(ERRS[m.code]||'That move was not allowed.');
  if(m.code==='code_taken'||m.code==='bad_code'){openSheet('friendSheet');$('customCode').focus()}break;
}}
function onWelcome(m){NET.ready=true;IAP_PACKS=Array.isArray(m.iap)?m.iap.filter(p=>p&&/^[a-z0-9_.]{1,64}$/.test(String(p.id))&&numOf(p.coins)>0).slice(0,8).map(p=>({id:String(p.id),coins:numOf(p.coins)})):null;setTimeout(()=>{iapStart();iapResend()},100);setTimeout(()=>NET.raw({t:'cup_list'}),50);NET.stats=m.stats||null;NET.roomTtl=Math.max(10,Number(m.roomTtl)||120);
 if(typeof m.pid==='string'&&/^[A-Za-z0-9-]{8,64}$/.test(m.pid)&&m.pid!==PID){PID=m.pid;ST.set('pid',PID)}
 const name=typeof m.name==='string'?m.name:'';
 if(RESTORE.prev){ // the player typed a recovery code: this device becomes that account
  const prev=RESTORE.prev;RESTORE.prev=null;ST.set('ft-restore','');
  if(name)commitName(name);ST.set('ft-nick-pending',false);
  adoptProg(validProg(m.prog)?m.prog:null);
  keyNote(`Welcome back, ${name}! The account this device had before used the code ${fmtKey(prev)}. Keep it if you want to switch back.`);
  $('keyIn').value='';paintKey();toast(`Welcome back, ${name}!`)}
 else{
  if(name&&name!==NAME){
   if(m.created){commitName(name);toast(`Your name is now ${name}. Someone else already uses the one you picked.`)}
   else if(ST.get('ft-nick-pending',false))sendName(NAME,ok=>{ST.set('ft-nick-pending',false);if(!ok){commitName(name);toast(`That name is taken, so you are still ${name}.`)}});
   else commitName(name)}
  else ST.set('ft-nick-pending',false);
  if(validProg(m.prog)){if(m.prog.xp>REC.xp)adoptProg(m.prog);else if(REC.xp>m.prog.xp)syncProg()}
  else if(REC.xp>0)syncProg()}
 netChanged();if(curTab==='leaders')requestLeaders()}
function onMatch(m){const v=m.view,o=m.opp;if(!v||typeof v!=='object'||!o)return;SS.set('ft-resume',typeof m.token==='string'?m.token:null);
 const opp={nick:String(o.name||'Opponent').slice(0,16),tr:Math.max(0,Number(o.tr)||0),lv:Math.max(1,Number(o.lv)||1),pid:String(o.pid||'').slice(0,64),bot:o.bot===true,tier:Math.max(0,Math.min(ARENAS.length-1,Math.floor(Number(o.tier))||0)),back:okItem(o.back,'back')?o.back:'stripes'};
 if(M.mode!=='online')M={mode:'online',oppEmoT:undefined};
 Object.assign(M,{opp,oppKey:'p:'+opp.pid,kind:m.kind==='room'?'room':m.kind==='bot'?'bot':m.kind==='cup'?'cup':'quick',cupM:Number.isInteger(m.cupMatch)?m.cupMatch:null,code:typeof m.code==='string'?m.code.replace(/[^A-Z]/g,''):null,
  oppHere:m.oppConnected!==false,oppLeft:false,earned:null,watchers:0,closed:false,reconnecting:false,iWantRm:null,theyWantRm:null});
 V=v;V.at=Date.now();pending=false;sel=null;
 $('roomTag').textContent=M.kind==='room'?'Room '+M.code:M.kind==='bot'?'Computer battle':M.kind==='cup'?(CUPS[M.code]?CUPS[M.code].name:'Cup match'):'Quick match';
 if($('game').hidden)show('game');
 renderGame()}
function netDown(wasReady){if(M.mode==='queue'||M.mode==='room'||M.mode==='cupwait'){goHome(true);toast('Lost connection to the game server.')}
 else if(M.mode==='online'){if(V&&V.ov){M.closed=true;updateRmNote()}else{M.reconnecting=true;if(V)renderGame()}}}
function netChanged(){const ok=NET.ready,s=NET.stats;
 ['quickBtn','create','join','joinCode'].forEach(id=>{const e=$(id);if(e)e.disabled=!ok});
 $('onlineLine').innerHTML=!SERVER_URL?'<span class="netdot"></span>Needs the game server':ok?`<span class="netdot on"></span>${s?Number(s.online)||0:1} online now`:'<span class="netdot wait"></span>Connecting…';
 const rt=$('roomTtlNote');if(rt){const m=Math.round((NET.roomTtl||120)/60*10)/10;rt.textContent=`Anyone with the code can join, so share it only with your friend. The room closes after ${m>=1?m+(m===1?' minute':' minutes'):(NET.roomTtl||120)+' seconds'} if nobody joins.`}
 $('roomNote').textContent=!SERVER_URL?'Open this page from the game server address to play online.':ok?(s?`${Number(s.online)||0} online · ${Number(s.searching)||0} searching · ${Number(s.playing)||0} in battle. Quick match uses your clock setting.`:'Connected.'):'Connecting to the game server…'}
function showWait(label,code,msg){show('wait');$('botOffer').hidden=true;$('waitLbl').textContent=label;$('waitCode').textContent=code||'';$('waitCode').hidden=!code;$('waitMsg').textContent=msg;
 $('waitShare').hidden=!code;$('waitTimer').hidden=!code;if(code)setupShare(code)}
/* ---- Sharing a room code ---- */
const inviteLink=code=>location.origin+location.pathname+'#'+code;
const inviteText=code=>`Join my Five Trails battle! Room code: ${code}\n${inviteLink(code)}`;
async function copyText(t){try{await navigator.clipboard.writeText(t);return true}catch(e){try{const a=document.createElement('textarea');a.value=t;a.setAttribute('readonly','');a.style.cssText='position:fixed;left:-9999px;top:0';document.body.appendChild(a);a.select();const ok=document.execCommand('copy');a.remove();return ok}catch(_){return false}}}
function setupShare(code){
 $('shWa').href='https://wa.me/?text='+encodeURIComponent(inviteText(code));
 $('shMore').hidden=typeof navigator.share!=='function';
 $('shCopy').onclick=async()=>{sfx('ui');toast(await copyText(inviteText(code))?'Invite copied. Paste it anywhere.':'Could not copy. Long-press the code instead.')};
 $('waitCode').onclick=async()=>{sfx('ui');toast(await copyText(code)?`Code ${code} copied.`:'Could not copy the code.')};
 $('shMore').onclick=()=>{sfx('ui');try{navigator.share({title:'Five Trails',text:`Join my Five Trails battle! Room code: ${code}`,url:inviteLink(code)}).catch(()=>{})}catch(e){}}}


/* ================= Cups: private round robin or knockout for 3 to 6 friends ================= */
let cupAwaitNew=false,cupAwaitJoin=null;
let CUPS={},cupOpen=null,cupFmtSel='rr',cupClkSel=300,cupPings={};
const cupFmtName=f=>f==='ko'?'Knockout':'Round robin';
const cupClkName=c=>c?`${c/60} min clock`:'No clock';
const cupInviteLink=code=>location.origin+location.pathname+'#cup-'+code;
const cupInviteText=c=>`Join my Five Trails cup "${c.name}"! Cup code: ${c.code}\n${cupInviteLink(c.code)}`;
function cleanCup(v){if(!v||typeof v!=='object'||!/^[A-Z]{6}$/.test(String(v.code||'')))return null;
 const int=(x,max)=>Number.isInteger(x)&&x>=0&&x<max?x:null,n=Array.isArray(v.players)?v.players.slice(0,6).map(p=>({n:String(p&&p.n||'?').slice(0,16),tr:numOf(p&&p.tr)})):[],np=n.length;
 const matches=Array.isArray(v.matches)?v.matches.slice(0,20).map(x=>({a:int(x&&x.a,np),b:int(x&&x.b,np),r:numOf(x&&x.r,9),draws:numOf(x&&x.draws,99),
  res:x&&x.res&&typeof x.res==='object'?{w:int(x.res.w,np),sa:Number(x.res.sa)||0,sb:Number(x.res.sb)||0,wo:!!x.res.wo}:null,
  ready:Array.isArray(x&&x.ready)?[!!x.ready[0],!!x.ready[1]]:[false,false],live:!!(x&&x.live),dueIn:x&&x.dueIn!=null?numOf(x.dueIn,1e8):null})):[];
 const table=Array.isArray(v.table)?v.table.slice(0,6).map(r=>({p:int(r&&r.p,np),pl:numOf(r&&r.pl),w:numOf(r&&r.w),d:numOf(r&&r.d),l:numOf(r&&r.l),pts:numOf(r&&r.pts),diff:Number(r&&r.diff)||0})).filter(r=>r.p!=null):[];
 return{code:v.code,name:String(v.name||'Cup').slice(0,24),format:v.format==='ko'?'ko':'rr',clock:numOf(v.clock,3600),state:['lobby','live','done'].includes(v.state)?v.state:'lobby',
  me:int(v.me,np),host:v.host===true,players:n,matches,table,champion:int(v.champion,np),runner:int(v.runner,np),
  feed:Array.isArray(v.feed)?v.feed.slice(0,12).map(l=>String(l).slice(0,90)):[],prize:v.prize&&typeof v.prize==='object'?{place:numOf(v.prize.place,9),coins:numOf(v.prize.coins)}:null,at:Date.now()}}
const myCupMatches=c=>c.me==null?[]:c.matches.map((x,i)=>({...x,i})).filter(x=>x.a===c.me||x.b===c.me);
const cupSide=(c,x)=>x.a===c.me?0:1;
const cupOpp=(c,x)=>x.a===c.me?x.b:x.a;
const cupOpenMatch=x=>x.a!=null&&x.b!=null&&!x.res;
function cupNeedsMe(c){if(c.state==='lobby')return c.host&&c.players.length>=3;return c.state==='live'&&myCupMatches(c).some(cupOpenMatch)}
function cupDue(c,x){return x.dueIn==null?null:Math.max(0,x.dueIn-(Date.now()-c.at)/1000)}
function cupPinged(c,x){const t=cupPings[c.code+':'+x.i];return t&&Date.now()-t<90000}
function onCup(v){const c=cleanCup(v);if(!c)return;const old=CUPS[c.code];CUPS[c.code]=c;
 if((cupAwaitNew&&!old&&c.host)||cupAwaitJoin===c.code){cupAwaitNew=false;cupAwaitJoin=null;sfx('join');closeSheets();openCup(c.code)}
 if(old&&old.state==='lobby'&&c.state==='live'){sfx('go');toast(`${c.name} has started! Play your matches.`)}
 renderCupBits(c.code)}
function renderCupBits(code){if(curTab==='cups')renderCups();if(cupOpen===code&&!$('cupSheet').hidden)renderCupSheet();paintCupDot()}
function paintCupDot(){const b=$('navCups');if(!b)return;const any=Object.values(CUPS).some(cupNeedsMe);b.classList.toggle('hasdot',any)}
function cupStatus(c){
 if(c.state==='lobby')return`Waiting to start · ${c.players.length} of 6 players`;
 if(c.state==='done')return`Winner: ${c.champion!=null?esc(c.players[c.champion].n):'—'}${c.prize?` · you got ${c.prize.coins} coins`:''}`;
 const mine=myCupMatches(c),open=mine.filter(cupOpenMatch),done=c.matches.filter(x=>x.res).length;
 return`${done} of ${c.matches.length} matches played${open.length?` · <b>${open.length} for you to play</b>`:''}`}
function renderCups(){const el=$('tab-cups');if(!el)return;
 const head=`<div class="pn"><div class="pn-h ol">Cups</div><p class="note">Private tournaments for 3 to 6 friends. Round robin or knockout, played whenever you are both online.</p>
  <div class="cupbtns"><button class="gb green" id="cupNewBtn" type="button">Create cup</button><button class="gb blue" id="cupJoinBtn" type="button">Join cup</button></div></div>`;
 let body;
 if(!SERVER_URL)body='<div class="pn pad"><p class="note">Cups need the game server. Open the game from its server address to create or join one.</p></div>';
 else if(!NET.ready)body='<div class="pn pad"><p class="note">Connecting to the game server…</p></div>';
 else{const list=Object.values(CUPS).sort((a,b)=>(a.state==='done')-(b.state==='done')||cupNeedsMe(b)-cupNeedsMe(a)||a.name.localeCompare(b.name));
  body=list.length?`<div class="cuplist">${list.map(c=>`<button class="cupcard ${c.state}" data-cup="${c.code}" type="button"><span class="cc-ic">${c.state==='done'?IC.cup:CUPIC}</span><span class="cc-t"><b>${esc(c.name)}</b><small>${cupFmtName(c.format)} · ${cupStatus(c)}${c.matches.some(x=>x.live)?' · <span class="livetag"><span class="livedot"></span>LIVE</span>':''}</small></span>${cupNeedsMe(c)?'<i class="dot" aria-label="Something for you to do"></i>':''}</button>`).join('')}</div>`
   :'<div class="pn pad"><p class="note" style="text-align:center">No cups yet. Create one and share the code with your friends.</p></div>'}
 el.innerHTML=head+body;
 $('cupNewBtn').onclick=()=>{sfx('ui');openCupNew()};$('cupJoinBtn').onclick=()=>{sfx('ui');$('cupCode').value='';$('cupJoinNote').textContent='';openSheet('cupJoinSheet')};
 el.querySelectorAll('[data-cup]').forEach(b=>b.onclick=()=>{sfx('ui');openCup(b.dataset.cup)})}
function openCupNew(){$('cupName').value=`${nick()}'s Cup`.slice(0,24);cupFmtSel='rr';cupClkSel=clkSet||300;paintCupNew();openSheet('cupNewSheet')}
function paintCupNew(){document.querySelectorAll('#cupFmt [data-fmt]').forEach(b=>b.setAttribute('aria-checked',String(b.dataset.fmt===cupFmtSel)));
 document.querySelectorAll('#cupClk [data-cclk]').forEach(b=>b.setAttribute('aria-checked',String(+b.dataset.cclk===cupClkSel)));
 $('cupFmtNote').textContent=cupFmtSel==='rr'?'Everyone plays everyone once. Win 3 points, draw 1. Three days to play.':'A bracket seeded by trophies; top seeds get a bye. Lose and you are out. A day per round.'}
function openCup(code){cupOpen=code;renderCupSheet();openSheet('cupSheet');if(NET.ready)NET.raw({t:'cup_get',code})}
function cupName(c,p){return p==null?'<i class="tbd">To be decided</i>':esc(c.players[p].n)+(p===c.me?' <span class="youtag">you</span>':'')}
function renderCupSheet(){const c=CUPS[cupOpen],body=$('cupBody');if(!body)return;if(!c){body.innerHTML='<p class="note">Loading…</p>';return}
 $('cupTitle').textContent=c.name;
 let h=`<div class="cupmeta"><span class="cfmt">${cupFmtName(c.format)}</span><span>${cupClkName(c.clock)}</span><button class="ccode" id="cupCodeBtn" type="button" aria-label="Copy cup code">${c.code}</button></div>`;
 if(c.state==='lobby'){
  h+=`<div class="cupplayers">${c.players.map((p,i)=>`<div class="cp ${i===c.me?'me':''}">${avatarHTML(p.n,1)}<span>${esc(p.n)}${i===0?' <small>host</small>':''}</span></div>`).join('')}${Array.from({length:6-c.players.length},()=>'<div class="cp empty"><span>Open seat</span></div>').join('')}</div>
   <div class="sharerow"><a class="gb green sm" id="cupWa" target="_blank" rel="noopener noreferrer" href="https://wa.me/?text=${encodeURIComponent(cupInviteText(c))}">WhatsApp</a><button class="gb blue sm" id="cupCopy" type="button">Copy invite</button></div>`;
  h+=c.host?`<button class="gb green" id="cupStart" type="button" ${c.players.length<3?'disabled':''}>Start the cup</button><p class="note" style="text-align:center">${c.players.length<3?`Needs at least 3 players (${c.players.length} so far).`:'Nobody can join once it starts.'}</p><button class="linkbtn" id="cupLeave" type="button">Cancel this cup</button>`
   :`<p class="note" style="text-align:center">Waiting for ${esc(c.players[0].n)} to start the cup.</p><button class="linkbtn" id="cupLeave" type="button">Leave this cup</button>`}
 else{
  if(c.state==='done')h+=`<div class="champ">${LAUREL}<div><small>Champion</small><b class="ol">${c.champion!=null?esc(c.players[c.champion].n):'—'}</b>${c.runner!=null?`<span>Runner-up: ${esc(c.players[c.runner].n)}</span>`:''}</div></div>${c.prize?`<p class="note" style="text-align:center">${c.prize.place===1?'You won the cup! The Champion’s Laurel card back is yours.':c.prize.place===2?'You finished second.':'Thanks for playing.'} ${c.prize.coins} coins went to your piggy bank.</p>`:''}`;
  h+=c.format==='rr'?cupTableHTML(c):cupBracketHTML(c);
  const mine=myCupMatches(c);
  const others=c.matches.map((x,i)=>({...x,i})).filter(x=>x.live&&x.a!==c.me&&x.b!==c.me);
  if(others.length)h+=`<div class="cupsec"><span class="livedot"></span>Live now</div><div class="cms">${others.map(x=>`<div class="cm open"><span class="cm-o"><b>${esc(c.players[x.a].n)}</b> vs <b>${esc(c.players[x.b].n)}</b></span>${cupWatchBit(c,x)}</div>`).join('')}</div>`;
  if(mine.length&&c.format==='rr')h+=`<div class="cupsec">Your matches</div><div class="cms">${mine.map(x=>cupMatchRow(c,x)).join('')}</div>`;
  if(c.feed.length)h+=`<div class="cupsec">Latest</div><ul class="cupfeed">${c.feed.slice(0,8).map(l=>`<li>${esc(l)}</li>`).join('')}</ul>`}
 body.innerHTML=h;
 const cb=$('cupCodeBtn');if(cb)cb.onclick=async()=>{sfx('ui');toast(await copyText(c.code)?`Cup code ${c.code} copied.`:'Could not copy the code.')};
 const cc=$('cupCopy');if(cc)cc.onclick=async()=>{sfx('ui');toast(await copyText(cupInviteText(c))?'Invite copied. Paste it in your group chat.':'Could not copy.')};
 const st=$('cupStart');if(st)st.onclick=()=>{sfx('ui');NET.send({t:'cup_start',code:c.code})};
 const lv=$('cupLeave');if(lv)lv.onclick=()=>{if(lv.dataset.armed){sfx('ui');NET.send({t:'cup_leave',code:c.code})}else{lv.dataset.armed='1';lv.textContent=c.host?'Tap again to cancel the cup':'Tap again to leave';sfx('select')}};
 body.querySelectorAll('[data-play]').forEach(b=>b.onclick=()=>{sfx('ui');if(NET.send({t:'cup_play',code:c.code,m:+b.dataset.play}))b.disabled=true});
 body.querySelectorAll('[data-watch]').forEach(b=>b.onclick=()=>{sfx('ui');if(NET.send({t:'watch',code:c.code,m:+b.dataset.watch}))b.disabled=true})}
function cupTableHTML(c){const rows=c.table.length?c.table:c.players.map((_,p)=>({p,pl:0,w:0,d:0,l:0,pts:0,diff:0}));
 return`<table class="cuptab"><thead><tr><th></th><th class="nm">Player</th><th>P</th><th>W</th><th>D</th><th>L</th><th>+/-</th><th>Pts</th></tr></thead><tbody>${rows.map((r,k)=>`<tr class="${r.p===c.me?'me':''} ${c.state==='done'&&k===0?'win':''}"><td>${k+1}</td><td class="nm">${esc(c.players[r.p].n)}</td><td>${r.pl}</td><td>${r.w}</td><td>${r.d}</td><td>${r.l}</td><td>${r.diff>0?'+':''}${r.diff}</td><td><b>${r.pts}</b></td></tr>`).join('')}</tbody></table>`}
function cupRoundName(c,r){const max=Math.max(...c.matches.map(x=>x.r));return r===max?'Final':r===max-1?'Semi-finals':'Round 1'}
function cupBracketHTML(c){const rounds=[...new Set(c.matches.map(x=>x.r))].sort((a,b)=>a-b);
 return`<div class="bracket">${rounds.map(r=>`<div class="bcol"><div class="bhead">${cupRoundName(c,r)}</div>${c.matches.map((x,i)=>({...x,i})).filter(x=>x.r===r).map(x=>{
   const line=(p,score,won)=>`<div class="bl ${won?'won':''} ${p!=null&&p===c.me?'me':''}"><span>${cupName(c,p)}</span>${x.res&&!x.res.wo?`<b>${score}</b>`:''}</div>`;
   const mine=x.a===c.me||x.b===c.me;
   return`<div class="bm ${mine?'mine':''}">${line(x.a,x.res?x.res.sa:0,x.res&&x.res.w===x.a)}${line(x.b,x.res?x.res.sb:0,x.res&&x.res.w===x.b)}${x.res&&x.res.wo?'<small>walkover</small>':x.draws&&!x.res?`<small>drawn ${x.draws}× · replay</small>`:''}${mine&&c.state==='live'?cupPlayBit(c,x):!mine?cupWatchBit(c,x):''}</div>`}).join('')}</div>`).join('')}</div>`}
function cupWatchBit(c,x){return x.live&&c.state==='live'?`<div class="cmplay"><button class="gb red sm" data-watch="${x.i}" type="button"><span class="livedot"></span>Watch live</button></div>`:''}
function cupPlayBit(c,x){if(!cupOpenMatch(x))return'';if(x.live)return`<div class="cmplay"><small class="livetag"><span class="livedot"></span>Playing now</small></div>`;const opp=c.players[cupOpp(c,x)].n,due=cupDue(c,x),side=cupSide(c,x);
 const live=cupPinged(c,x),theyReady=x.ready[1-side];
 return`<div class="cmplay"><button class="gb green sm ${live?'pulse':''}" data-play="${x.i}" type="button">${live?`Play now · ${esc(opp)} is waiting`:'Play'}</button><small>${theyReady&&!live?`${esc(opp)} showed up · `:''}${due!=null?`due in ${fmtSpan(due)}`:''}</small></div>`}
function cupMatchRow(c,x){const o=cupOpp(c,x),side=cupSide(c,x),opp=o!=null?esc(c.players[o].n):'?';let st='';
 if(x.res){const mine=side===0?x.res.sa:x.res.sb,theirs=side===0?x.res.sb:x.res.sa;
  st=x.res.wo?(x.res.w===c.me?'Won (they did not show)':x.res.w==null?'Not played':'Lost (you did not show)'):x.res.w===c.me?`Won ${mine}–${theirs}`:x.res.w==null?`Draw ${mine}–${theirs}`:`Lost ${mine}–${theirs}`}
 return`<div class="cm ${x.res?(x.res.w===c.me?'w':x.res.w==null?'d':'l'):'open'}"><span class="cm-o">vs <b>${opp}</b></span>${x.res?`<span class="cm-s">${st}</span>`:cupPlayBit(c,x)}</div>`}

/* ================= App state ================= */
let M={mode:'home'};          // mode: home | ai | queue | room | online
let V=null, prevGid=null, prevSeq=0, aiT=null, sel=null, pending=false, PS=null;
function show(id){for(const s of ['home','wait','game'])$(s).hidden=s!==id;$('over').hidden=true;closeSheets();closeEmo();musicSync()}

/* ================= Lobby ================= */
let clkSet=ST.get('ft-clock',300);
const clockMs=()=>clkSet*1000;
function renderTop(){const L=lvlInfo(REC.xp);
 $('topProf').innerHTML=`${avatarHTML(nick(),L.lv)}<div class="pinfo"><div class="nm ol">${esc(nick())}</div><div class="bar xp thin"><i style="width:${L.into/L.need*100}%"></i><b class="ols">${L.into} / ${L.need} XP</b></div></div>`;
 $('topTr').innerHTML=`${TROPHY}<span class="ol">${REC.tr}</span>`}
function renderHome(){
 renderTop();
 const ai=arenaIdx(REC.tr),A=ARENAS[ai],N=ARENAS[ai+1];const pct=N?(REC.tr-A.at)/(N.at-A.at)*100:100;
 const ac_=$('arenaCard');ac_.style.setProperty('--ag1',A.g[0]);ac_.style.setProperty('--ag2',A.g[1]);
 ac_.innerHTML=`<div class="rays"></div><div class="emb">${emblem(ai,118)}</div><div class="an ol">${A.n}</div><div class="asub ols">Arena ${ai+1} of ${ARENAS.length}</div>
  <div class="tprog">${TROPHY}<div class="bar"><i style="width:${pct}%"></i><b class="ols">${N?`${REC.tr} / ${N.at}`:`${REC.tr} · Top arena`}</b></div></div>`;
 $('aiTier').textContent=`vs Computer · ${AI_TIER[ai]}`;
 netChanged();
 renderRecords();renderRewards();maybeShowReward();
}
function renderRecords(){const T=tot(),n=T.w+T.l+T.d,ai=arenaIdx(REC.tr);
 const opps=Object.values(REC.opp).sort((a,b)=>(b.w+b.l+b.d)-(a.w+a.l+a.d)).slice(0,8);
 $('tab-records').innerHTML=`
 <div class="pn"><div class="pn-h ol">Career</div><div class="stats">
  <div class="stat"><span>Wins</span><b>${T.w}</b></div><div class="stat"><span>Losses</span><b>${T.l}</b></div>
  <div class="stat"><span>Win rate</span><b>${n?Math.round(T.w/n*100)+'%':'–'}</b></div><div class="stat"><span>Best trophies</span><b>${REC.best}</b></div></div>
  <div class="kv" style="margin-top:8px"><span>vs Computer</span><span>${wld(REC.ai)}</span></div><div class="kv"><span>vs Players</span><span>${wld(REC.pvp)}</span></div></div>
 <div class="pn"><div class="pn-h ol">Head to head</div>${opps.length?opps.map(o=>`<div class="kv"><span>${avatarHTML(o.name).replace('class="av"','class="av" style="width:28px;height:28px;font-size:15px;border-width:2px;border-radius:8px"')}${esc(o.name)}</span><span>${wld(o)}</span></div>`).join(''):'<p class="note">Beat a friend in a room and your rivalry shows up here.</p>'}</div>
 <div class="pn"><div class="pn-h ol">Recent battles</div>${REC.recent.length?REC.recent.slice(0,8).map(r=>`<div class="kv"><span><span class="rtag ${r.r}">${r.r.toUpperCase()}</span>vs ${esc(r.o)}</span><span>${r.me} – ${r.them}${r.dt!=null?` <span class="dt">${r.dt>0?'+':''}${r.dt}</span>`:''}</span></div>`).join(''):'<p class="note">No battles yet. Your first one is a tap away.</p>'}</div>
 <div class="pn"><div class="pn-h ol">Arena road</div><div class="road">${ARENAS.map((A,i)=>`<div class="rd ${REC.tr>=A.at?'open':'lock'} ${i===ai?'cur':''}">${emblem(i,40,'embs')}<div><b>${A.n}</b><span>${TROPHY}${A.at} trophies · ${AI_TIER[i]} computer · ${CARDSETS[i].n} cards</span></div>${i===ai?'<em>You are here</em>':REC.tr>=A.at?'':'<em class="lk">Locked</em>'}</div>`).join('')}</div></div>`}
function renderGuide(){
 const ex=(cards,math)=>`<div class="ex"><div class="cards">${cards.map(id=>mini(id,34)).join('')}</div><div class="math">${math}</div></div>`;
$('tab-guide').innerHTML=`
 <div class="pn gsec"><div class="pn-h ol">Each turn</div>
  <p><b>1. Play</b> a card to its color's trail, or discard it to that color's pile.</p>
  <p><b>2. Draw</b> from the deck or any discard pile (not the one you just discarded to).</p>
  <p>Cards in a trail must climb. Wagers go first. The battle ends when the deck runs out.</p></div>
 <div class="pn gsec"><div class="pn-h ol">Scoring</div>
  <p>Trail score = (sum − 20) × (1 + wagers). 8+ cards: +20. Unstarted trails: 0.</p>
  ${ex([cid(0,3),cid(0,6),cid(0,8),cid(0,10)],'Desert: 27 − 20 = <b>7</b>')}
  ${ex([cid(1,0,0),cid(1,4),cid(1,7),cid(1,9)],'1 wager: (20 − 20) × 2 = <b>0</b>')}
  ${ex([cid(2,0,0),cid(2,0,1),cid(2,5),cid(2,7),cid(2,9),cid(2,10)],'2 wagers: (31 − 20) × 3 = <b>33</b>')}
  ${ex([cid(3,0,0),cid(3,2),cid(3,4)],'1 wager: (6 − 20) × 2 = <b>−28</b>')}</div>
 <div class="pn gsec"><div class="pn-h ol">Tricks · one of each per battle</div>
  <p><span class="pwb">SABOTAGE</span> Knocks a random rival card to the discards. They draw a new one.</p>
  <p><span class="pwb">EVIL EYE</span> Shows you one random rival card until they play it. They see which. Locked in the last 10 deck cards and on a rushed turn.</p>
  <p><span class="pwb">RUSH</span> A secret. They are not told until their turn starts, then get 10 seconds. If time runs out, a random card is discarded and they draw.</p>
  <p class="note">Tap a button twice to use it. Used tricks are struck out.</p></div>
 <div class="pn gsec"><div class="pn-h ol">Trophies</div>
  <p>Computer: +12 / −8. Players: +30 / −20. You never drop below an arena you reached.</p>
  <p>Each arena sharpens the computer and unlocks a card style (Settings). Battles earn XP.</p>
  <p>With a clock, only your own turns count down.</p></div>
 <div class="pn gsec"><div class="pn-h ol">Cups</div>
  <p>A private tournament for 3 to 6 friends. Create one in the Cups tab and share the code.</p>
  <p><b>Round robin:</b> everyone plays everyone once; win 3 points, draw 1; three days to play. <b>Knockout:</b> a bracket seeded by trophies; a day per round; a draw is replayed.</p>
  <p>Matches being played show <b>Watch live</b> to everyone in the cup. Spectators see the board, never anyone’s hand.</p>
  <p>Tap Play when you are ready; your friend is told. Miss the deadline and whoever showed up wins. Champion: 100 coins and the Champion’s Laurel card back. Cup games never change trophies.</p></div>
 <div class="pn gsec"><div class="pn-h ol">Rewards</div>
  <p>Every finished online battle pays coins: 15 for a win, 8 for a draw, 5 for a loss (half against the computer), up to 150 a day.</p>
  <p>A free chest every few hours (bank of 2), plus daily and weekly quests. Saved on the server.</p>
  <p>Quick matches feed a monthly season; top players win coins when it ends.</p>
  <p>No one around? Play the computer: unranked, still counts for quests.</p>
  <p>Spend coins in the Shop (tap your coins): table themes for your screen, and card backs your rival sees. Three of each are free.</p>
  <button class="gb blue sm" id="replayIntro2" style="align-self:flex-start">Replay intro</button></div>`;
 $('replayIntro2').onclick=()=>{sfx('ui');openIntro()}}
let curTab='battle';
let lbScope=ST.get('ft-lbscope','season')==='all'?'all':'season';
const LBS={season:null,all:null};
function setTab(t){curTab=t;document.querySelectorAll('.nav button').forEach(b=>b.setAttribute('aria-selected',String(b.dataset.tab===t)));
 for(const k of ['battle','leaders','cups','records','guide'])$('tab-'+k).hidden=k!==t;if(t==='leaders'){renderLeaders();requestLeaders()}if(t==='cups'){renderCups();if(NET.ready)NET.raw({t:'cup_list'})}try{scrollTo({top:0})}catch(e){}}
/* ---- Leaderboard (ranked online quick matches, kept by the server): this season, or all time ---- */
function requestLeaders(){if(NET.ready)NET.raw({t:'leaderboard',season:lbScope==='season'})}
function cleanLB(m){const n=v=>Math.max(0,Math.floor(Number(v))||0);
 const top=Array.isArray(m.top)?m.top.slice(0,50).map(r=>({n:String(r&&r.n||'').slice(0,16),tr:n(r&&r.tr),w:n(r&&r.w),l:n(r&&r.l),d:n(r&&r.d),me:!!(r&&r.me)})):[];
 const me=m.me&&typeof m.me==='object'?{rank:n(m.me.rank),n:String(m.me.n||'').slice(0,16),tr:n(m.me.tr),w:n(m.me.w),l:n(m.me.l),d:n(m.me.d)}:null;
 const season=m.season&&typeof m.season==='object'?{name:String(m.season.name||'').slice(0,30),endsIn:n(m.season.endsIn)}:null;
 return{scope:m.scope==='season'?'season':'all',top,me,total:n(m.total),season,at:Date.now()}}
function renderLeaders(){const el=$('tab-leaders');if(!el)return;
 const LB=LBS[lbScope],sInfo=(LB&&LB.season)||(ME&&ME.season?{name:ME.season.name,endsIn:left(ME.season.endsIn)}:null);
 const sLeft=LB&&LB.season?Math.max(0,LB.season.endsIn-(Date.now()-LB.at)/1000):(sInfo?sInfo.endsIn:0);
 const intro=lbScope==='season'?`${sInfo?`<b>${esc(sInfo.name)}</b> · ends in ${fmtSpan(sLeft)}. `:''}Top finishers earn coins. `:'Everyone who has played ranked. ';
 const head=`<div class="pn"><div class="pn-h ol">Leaderboard</div>
  <div class="seg-b lbscope" role="radiogroup" aria-label="Leaderboard period"><button role="radio" data-lbs="season" aria-checked="${lbScope==='season'}">This season</button><button role="radio" data-lbs="all" aria-checked="${lbScope==='all'}">All time</button></div>
  <p class="note" style="margin-top:10px">${intro}Online quick matches only: +30 win, −20 loss.</p></div>`;
 const bind=()=>el.querySelectorAll('[data-lbs]').forEach(b=>b.onclick=()=>{if(lbScope===b.dataset.lbs)return;sfx('ui');lbScope=b.dataset.lbs;ST.set('ft-lbscope',lbScope);renderLeaders();requestLeaders()});
 if(!SERVER_URL){el.innerHTML=head+'<div class="pn pad"><p class="note">The leaderboard needs the game server. Open this page from the game server address.</p></div>';bind();return}
 if(!LB){el.innerHTML=head+`<div class="pn pad"><p class="note">${NET.ready?'Loading the leaderboard…':'Connecting to the game server…'}</p></div>`;bind();return}
 const row=(r,rank)=>`<div class="lb${r.me?' me':''}"><span class="lbr r${rank<=3?rank:0}">${rank}</span>${avatarHTML(r.n)}<div class="lbn"><b>${esc(r.n)}${r.me?' <em>You</em>':''}</b><span>${wld(r)}</span></div><div class="lbt">${TROPHY}<span>${r.tr}</span></div></div>`;
 const me=LB.me,per=lbScope==='season'?'this season':'';
 const mine=me?`<div class="pn pad lbme"><div class="lbme-t">Your rank${per?' '+per:''}</div><div class="lbme-r"><b class="ol">#${me.rank}</b><span>of ${LB.total} ranked player${LB.total===1?'':'s'}</span></div><div class="lbme-s">${TROPHY}${me.tr} · ${wld(me)}</div></div>`
  :(LB.top.length?`<div class="pn pad lbme"><p class="note">You are not ranked${per?' '+per:''} yet. Win or lose an online quick match to get on the board.</p></div>`:'');
 const inTop=me&&LB.top.some(r=>r.me);
 el.innerHTML=head+mine+`<div class="pn pad lblist">${LB.top.length?LB.top.map((r,i)=>row(r,i+1)).join(''):`<p class="note">Nobody is ranked${per?' '+per:''} yet. Win or lose an online quick match to be the first on the board.</p>`}${me&&!inTop?`<div class="lbgap">···</div>${row({...me,me:true},me.rank)}`:''}</div>
  <button class="gb blue sm" id="lbRefresh" style="align-self:center">Refresh</button>`;
 bind();
 const rb=$('lbRefresh');if(rb)rb.onclick=()=>{sfx('ui');requestLeaders()}}

/* ================= Rewards: coins, free chest, quests (all kept by the server) ================= */
let ME=null;
const PIGGY=`<svg viewBox="0 0 44 36" class="pg" aria-hidden="true"><path d="M33 13.5c3.2-.4 5.4 1.2 5.6 3.6.2 2.6-1.8 4.4-4.4 4.4" fill="#ff8fbf" stroke="${OLC}" stroke-width="2"/><path d="M12 9.5 10.2 4.8c-.3-.9.6-1.6 1.4-1.1l4.6 3" fill="#ff8fbf" stroke="${OLC}" stroke-width="2" stroke-linejoin="round"/><ellipse cx="21" cy="19" rx="15.5" ry="11.5" fill="#ffa8cd" stroke="${OLC}" stroke-width="2"/><path d="M10 27.5v4.2h4.6v-2.6M25 29.2v2.5h4.6v-4.4" fill="#ff8fbf" stroke="${OLC}" stroke-width="2" stroke-linejoin="round"/><ellipse cx="7.2" cy="19.6" rx="3.6" ry="3.2" fill="#ff8fbf" stroke="${OLC}" stroke-width="2"/><circle cx="6.1" cy="19.4" r=".9" fill="${OLC}"/><circle cx="8.5" cy="19.4" r=".9" fill="${OLC}"/><circle cx="13.8" cy="14.6" r="1.7" fill="${OLC}"/><circle cx="13.3" cy="14" r=".5" fill="#fff"/><path d="M26.5 15.5c2-1.2 4.2-1.2 6 .2" stroke="#ff6fa8" stroke-width="1.6" fill="none" stroke-linecap="round" opacity=".7"/><rect class="pg-slot" x="17" y="7.2" width="9" height="2.8" rx="1.4" fill="${OLC}"/></svg>`;
/* Coins never just appear: they fly in and drop into the piggy bank, which wiggles while the
   balance counts up. The target is the piggy in the lobby, a piggy on the result screen, or a
   piggy that pops up when a sheet covers the lobby. */
let coinAnim=null;
function piggyTarget(){const box=$('coinBox');const free=box&&!$('home').hidden&&!box.closest('[hidden]')&&!document.querySelector('.sheet:not([hidden])')&&box.getBoundingClientRect().width;
 if(free)return{get pig(){return box.querySelector('.pg')},set:v=>{const b=box.querySelector('.ol');if(b)b.textContent=v},done:()=>{}};
 let p=$('piggyPop');if(!p){p=document.createElement('div');p.id='piggyPop';p.className='piggypop';p.setAttribute('aria-hidden','true');document.body.appendChild(p)}
 clearTimeout(p._t);p.innerHTML=PIGGY+'<b class="ol"></b>';p.classList.remove('bye');p.hidden=false;
 return{pig:p.querySelector('.pg'),set:v=>{p.querySelector('b').textContent=v},done:()=>{p._t=setTimeout(()=>{p.classList.add('bye');p._t=setTimeout(()=>{p.hidden=true},380)},900)}}}
function depositCoins(amount,fromEl,base,tgt){amount=Math.max(0,Math.round(amount));if(!amount)return;
 const T=tgt||piggyTarget();if(!T||!T.pig)return;const start=base==null?(ME?ME.coins-amount:0):base;
 coinAnim={cur:start};T.set&&T.set(start);
 const finish=()=>{const end=start+amount;T.set&&T.set(end);coinAnim=null;const b=T.pig.getBoundingClientRect();if(b.width&&!reduced)burst(b.left+b.width/2,b.top+b.height*.25,['#ffd23a','#fff','#ff8fbf'],16,{sp:5});T.done&&T.done();if(!tgt)renderRewards()};
 const pr=T.pig.getBoundingClientRect(),fr=fromEl&&fromEl.getBoundingClientRect&&fromEl.getBoundingClientRect();
 const fx=fr&&fr.width?fr.left+fr.width/2:innerWidth/2,fy=fr&&fr.width?fr.top+fr.height/2:innerHeight*.45;
 if(reduced||!pr.width||typeof document.body.animate!=='function'){finish();sfx('coin');return}
 const sx=pr.left+pr.width*.49,sy=pr.top+pr.height*.22,n=Math.min(12,Math.max(3,Math.ceil(amount/5)));let landed=0;
 for(let i=0;i<n;i++){const d=document.createElement('div');d.className='flyico coinfly';d.innerHTML=COIN;document.body.appendChild(d);
  const ox=fx+(Math.random()-.5)*60,oy=fy+(Math.random()-.5)*30,peak=Math.min(oy,sy)-70-Math.random()*50,mx=(ox+sx)/2+(Math.random()-.5)*80;
  const an=d.animate([{transform:`translate(${ox}px,${oy}px) scale(.3) rotateY(0deg)`,opacity:0},{transform:`translate(${ox}px,${oy-26}px) scale(1.1) rotateY(180deg)`,opacity:1,offset:.16},
   {transform:`translate(${mx}px,${peak}px) scale(1) rotateY(540deg)`,opacity:1,offset:.55},{transform:`translate(${sx}px,${sy-14}px) scale(.8) rotateY(900deg)`,opacity:1,offset:.86},
   {transform:`translate(${sx}px,${sy+4}px) scale(.35,.12) rotateY(900deg)`,opacity:0}],{duration:1000,delay:i*110,easing:'cubic-bezier(.45,0,.55,1)',fill:'both'});
  an.onfinish=()=>{d.remove();landed++;const v=start+Math.round(amount*landed/n);coinAnim&&(coinAnim.cur=v);T.set&&T.set(v);
   T.pig.classList.remove('gulp');void T.pig.getBoundingClientRect();T.pig.classList.add('gulp');sfx('coin');if(landed===n)finish()}}}
const COIN=`<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9.5" fill="#ffd23a" stroke="${OLC}" stroke-width="2"/><circle cx="12" cy="12" r="5.6" fill="none" stroke="#c77400" stroke-width="1.8"/><path d="M12 8.8v6.4" stroke="#c77400" stroke-width="1.8" stroke-linecap="round"/></svg>`;
const numOf=(v,max=1e9)=>Math.min(max,Math.max(0,Math.floor(Number(v))||0));
function cleanMe(m){
 const q=a=>Array.isArray(a)?a.slice(0,3).map(x=>({slot:String(x&&x.slot||'').slice(0,3),text:String(x&&x.text||'').slice(0,60),need:Math.max(1,numOf(x&&x.need)),have:numOf(x&&x.have),coins:numOf(x&&x.coins),claimed:!!(x&&x.claimed)})):[];
 const c=m.chest&&typeof m.chest==='object'?m.chest:{};
 const r=m.reward&&typeof m.reward==='object'?{season:String(m.reward.season||'').slice(0,30),rank:numOf(m.reward.rank),coins:numOf(m.reward.coins)}:null;
 return{coins:numOf(m.coins),chest:{n:numOf(c.n,9),max:Math.max(1,numOf(c.max,9)),nextIn:c.nextIn==null?null:numOf(c.nextIn,1e7),every:numOf(c.every,1e7),coins:numOf(c.coins)},
  daily:q(m.daily),weekly:q(m.weekly),dailyIn:numOf(m.dailyIn,1e7),weeklyIn:numOf(m.weeklyIn,1e8),
  season:{name:String(m.season&&m.season.name||'').slice(0,30),endsIn:numOf(m.season&&m.season.endsIn)},reward:r,shop:cleanShop(m.shop),at:Date.now()}}
function cleanShop(x){if(!x||typeof x!=='object'||!Array.isArray(x.items))return null;
 const items=x.items.filter(i=>i&&okItem(i.id,i.kind)).map(i=>({id:i.id,kind:i.kind,price:numOf(i.price),tr:numOf(i.tr),cup:i.cup===true,have:i.have===true}));
 return{items,table:okItem(x.table,'table')?x.table:'classic',back:okItem(x.back,'back')?x.back:'stripes'}}
/* Seconds left of a countdown the server reported `secs` seconds ago. */
const left=secs=>Math.max(0,secs-(Date.now()-(ME?ME.at:Date.now()))/1000);
function fmtSpan(s){s=Math.max(0,Math.floor(s));const d=Math.floor(s/86400),h=Math.floor(s%86400/3600),m=Math.floor(s%3600/60);return d>=2?`${d} days`:d===1?`1 day ${h} h`:h>=1?`${h} h ${m} min`:`${Math.max(1,m)} min`}
function fmtClock(s){s=Math.ceil(Math.max(0,s));const h=Math.floor(s/3600),m=Math.floor(s%3600/60),r=s%60;return(h?h+':'+String(m).padStart(2,'0'):m)+':'+String(r).padStart(2,'0')}
let chestState=null,meAsked=false;
function renderRewards(){const el=$('rewards');if(!el)return;
 if(!ME||!SERVER_URL){el.hidden=true;return}
 el.hidden=false;meAsked=false;
 $('coinBox').innerHTML=`<span class="cb-row">${PIGGY}<span class="ol">${coinAnim?coinAnim.cur:ME.coins}</span></span><small>SHOP</small>`;
 const c=ME.chest,b=$('chestBtn');chestState=c.n>0?'ready':'wait';
 if(c.n>0){b.className='gb green chestbtn ready';b.innerHTML=`<span>Open chest${c.n>1?' ×'+c.n:''}</span><small>+${c.coins} coins</small>`}
 else{b.className='gb chestbtn wait';b.innerHTML=`<span>Free chest</span><small class="cb-s">${c.nextIn==null?'':'in '+fmtClock(left(c.nextIn))}</small>`}
 const claimable=[...ME.daily,...ME.weekly].filter(x=>x.have>=x.need&&!x.claimed).length,dn=ME.daily.filter(x=>x.claimed).length;
 $('questBtn').innerHTML=`<span>Quests</span><small>${dn} of ${ME.daily.length} today</small>${claimable?'<i class="dot" aria-label="Rewards to collect"></i>':''}`}
/* Once a second: only the countdown text changes, so the buttons are never rebuilt under a tapping finger. */
let lastRewSec=0;
function tickRewards(){if(!ME||$('home').hidden)return;const sec=Math.floor(Date.now()/1000);if(sec===lastRewSec)return;lastRewSec=sec;
 const c=ME.chest;
 if(chestState==='wait'&&c.nextIn!=null){const l=left(c.nextIn),s=$('chestBtn').querySelector('.cb-s');
  if(s)s.textContent='in '+fmtClock(l);
  if(l<=0&&!meAsked){meAsked=true;setTimeout(()=>NET.raw({t:'me'}),600)}}
 if(!$('questSheet').hidden)renderQuests()}
function renderQuests(){if(!ME)return;
 const row=x=>{const done=x.have>=x.need;return `<div class="q${done&&!x.claimed?' done':''}${x.claimed?' got':''}"><b>${esc(x.text)}</b><div class="bar"><i style="width:${Math.min(100,x.have/x.need*100)}%"></i><b class="ols">${x.have} / ${x.need}</b></div>
  <div class="qr"><span class="coinchip">${COIN}${x.coins}</span>${x.claimed?'<span class="rtag w">Done</span>':done?`<button class="gb green sm" data-claim="${esc(x.slot)}" type="button">Claim</button>`:''}</div></div>`};
 $('questBody').innerHTML=`<div class="qs"><div class="qh">Today<span>New quests in ${fmtSpan(left(ME.dailyIn))}</span></div>${ME.daily.map(row).join('')}
  <div class="qh">This week<span>New quests in ${fmtSpan(left(ME.weeklyIn))}</span></div>${ME.weekly.map(row).join('')}
  <p class="note">Online, friend and computer battles count. Quitting early does not.</p></div>`}
function onClaimed(m){const coins=numOf(m.coins);sfx('coin');
 const el=m.what==='chest'?$('chestBtn'):document.querySelector('#questSheet:not([hidden]) .q.done')||$('questBtn');
 if(el&&el.getBoundingClientRect().width){const[x,y]=centerOf(el);burst(x,y,['#ffd23a','#fff','#ffaa00'],22,{sp:7})}
 depositCoins(coins,el,ME?ME.coins:0);
 callout(`+${coins} coins`,'#ffd23a',m.what==='chest'?'Free chest':'Quest done')}
let seasonShown=null;
function maybeShowReward(){const r=ME&&ME.reward;if(!r||$('home').hidden||!$('intro').hidden)return;
 const key=r.season+':'+r.rank;if(seasonShown===key)return;seasonShown=key;
 $('seasonMsg').textContent=`${r.season} is over. You finished #${r.rank} on the leaderboard and earned ${r.coins} coins!`;
 NET.raw({t:'ack_season'});openSheet('seasonSheet');sfx('levelup');confetti();setTimeout(()=>depositCoins(r.coins,$('seasonMsg'),Math.max(0,ME.coins-r.coins)),700)}
/* ---- Reminders: a nudge when the next free chest is ready. Needs the iPhone app (local notifications). ---- */
let remOn=ST.get('ft-remind',false)===true;
const localNotes=()=>{try{return window.Capacitor&&window.Capacitor.Plugins&&window.Capacitor.Plugins.LocalNotifications||null}catch(e){return null}};
function paintRem(){const t=$('remTog'),n=$('remNote'),ln=localNotes();t.setAttribute('aria-checked',String(remOn&&!!ln));t.disabled=!ln;
 n.textContent=ln?'Nudge when your chest is ready.':'Available in the iPhone app.'}
async function scheduleReminders(){const ln=localNotes();if(!ln)return;
 try{await ln.cancel({notifications:[{id:1001}]})}catch(e){}
 if(!remOn||!ME)return;const c=ME.chest;if(c.n>=c.max||c.nextIn==null)return;
 const at=new Date(Date.now()+Math.max(5,left(c.nextIn))*1000);
 try{await ln.schedule({notifications:[{id:1001,title:'Five Trails',body:c.n===0?'Your free chest is ready!':'Your chest bank is about to be full. Open one so it keeps filling.',schedule:{at}}]})}catch(e){}}
async function toggleReminders(){const ln=localNotes();if(!ln)return;
 if(!remOn){try{const p=await ln.requestPermissions();if(!p||p.display!=='granted'){toast('Allow notifications for Five Trails in your phone settings to get reminders.');return}}catch(e){return}}
 remOn=!remOn;ST.set('ft-remind',remOn);paintRem();scheduleReminders();sfx('ui')}
function paintPicker(){document.querySelectorAll('#clkpick button').forEach(b=>b.setAttribute('aria-checked',String(+b.dataset.clk===clkSet)))}

/* Sheets */
function openSheet(id){closeSheets();$(id).hidden=false;sfx('swoosh');const f=$(id).querySelector('input,button.gb:not(.xbtn)');if(f&&id!=='leaveSheet')setTimeout(()=>{try{f.focus({preventScroll:true})}catch(e){}},50)}
function closeSheets(){document.querySelectorAll('.sheet').forEach(s=>{if(s.id!=='intro')s.hidden=true})}

/* Intro */
let introI=0;
const INTRO=[
 {t:'Welcome, explorer!',v:()=>`<div style="position:relative;width:240px;height:130px">${[cid(0,10),cid(1,9),cid(2,8),cid(3,7),cid(4,6)].map((id,i)=>mini(id,58,`position:absolute;left:91px;bottom:0;transform-origin:50% 150%;transform:rotate(${(i-2)*13}deg)`)).join('')}</div>`,p:'Five colored trails, one shared deck. Score more than your rival.'},
 {t:'Always climb',v:()=>[cid(1,3),cid(1,6),cid(1,9)].map((id,i)=>mini(id,56,`transform:translateY(${(1-i)*16}px)`)).join(''),p:'Each card must be higher than the last in its trail.'},
 {t:'Bet bold',v:()=>[cid(3,0,0),cid(3,7),cid(3,10)].map(id=>mini(id,56)).join(''),p:'Each trail costs 20 to start. Wagers go first and multiply it, win or lose.'},
 {t:'Three tricks per battle',v:()=>`<div class="trk"><div><span class="pwb">SABOTAGE</span><small>Knocks a random rival card away</small></div><div><span class="pwb">EVIL EYE</span><small>Shows you one random rival card</small></div><div><span class="pwb">RUSH</span><small>A secret: they get only 10 seconds</small></div></div>`,p:'One of each per battle. Tap twice to use.'},
 {t:"What's your name?",name:true}
];
function openIntro(){introI=0;closeSheets();$('intro').hidden=false;renderIntro()}
function renderIntro(){const s=INTRO[introI];const dots=`<div class="dots">${INTRO.map((_,i)=>`<i class="${i===introI?'on':''}"></i>`).join('')}</div>`;
 $('introPn').innerHTML=s.name?`<div class="islide"><h2>${s.t}</h2><div class="ivis">${avatarHTML(NAME||'E',null).replace('class="av"','class="av" style="width:84px;height:84px;font-size:44px;border-radius:22px"')}</div>
   <input id="introName" maxlength="16" placeholder="Explorer" value="${esc(NAME)}" aria-label="Your name" style="width:100%;text-align:center"><p class="note" id="introNote" aria-live="polite">Unique name picked for you. Change it any time.</p>${dots}
   <div class="introbtns"><button class="gb go" id="introGo">Let's go!</button></div></div>`
 :`<div class="islide"><h2>${s.t}</h2><div class="ivis">${s.v()}</div><p>${s.p}</p>${dots}<div class="introbtns"><button class="gb gray" id="introSkip">Skip</button><button class="gb green" id="introNext">Next</button></div></div>`;
 if(s.name){const inp=$('introName');inp.oninput=()=>{const av=$('introPn').querySelector('.av');if(av)av.firstChild.textContent=((inp.value||'E').trim()[0]||'E').toUpperCase()};
  inp.onkeydown=e=>{if(e.key==='Enter')$('introGo').click()};
  $('introGo').onclick=()=>{const fin=()=>{ST.set('ft-intro',true);$('intro').hidden=true;$('nick').value=NAME;sfx('go');renderHome();burst(innerWidth/2,innerHeight/2,['#ffd23a','#7dffa0','#4f9dff','#ff5d6c'],30,{sp:8})};
   if(!cleanNick(inp.value)){fin();return}
   trySetName(inp.value,(ok,msg)=>{if(!ok){const n=$('introNote');if(n){n.textContent=msg;n.classList.add('bad')}sfx('bad');return}fin()})}}
 else{$('introNext').onclick=()=>{sfx('ui');introI++;renderIntro()};$('introSkip').onclick=()=>{sfx('ui');introI=INTRO.length-1;renderIntro()}}}

/* ================= Rendering the battle ================= */
const rectOf=el=>el?el.getBoundingClientRect():null;
function snap(){const m=new Map();document.querySelectorAll('#game [data-id]').forEach(el=>m.set(el.dataset.id,{r:el.getBoundingClientRect(),h:el.outerHTML}));return m}
function animateFrom(el,from,delay=0){if(reduced||!from)return;const to=el.getBoundingClientRect();
 const dx=from.left-to.left,dy=from.top-to.top;if(Math.abs(dx)<1&&Math.abs(dy)<1&&Math.abs(from.width-to.width)<1)return;
 const sx=from.width/to.width,sy=from.height/to.height;el.style.zIndex=40;
 const a=el.animate([{transform:`translate(${dx}px,${dy}px) scale(${sx},${sy})`,transformOrigin:'0 0'},{transform:'none',transformOrigin:'0 0'}],{duration:440,delay,easing:'cubic-bezier(.2,.8,.2,1)',fill:'backwards'});
 a.onfinish=()=>{el.style.zIndex=''}}
function ghost(html,from,to,fade){if(reduced||!from||!to)return;const g=document.createElement('div');g.innerHTML=html;const el=g.firstElementChild;el.classList.add('fly');
 Object.assign(el.style,{left:from.left+'px',top:from.top+'px',width:from.width+'px',height:from.height+'px'});document.body.appendChild(el);
 const a=el.animate([{transform:'none',opacity:1},{transform:`translate(${to.left-from.left}px,${to.top-from.top}px) scale(${to.width/from.width},${to.height/from.height})`,opacity:fade?0:1}],{duration:460,easing:'cubic-bezier(.2,.8,.2,1)',fill:'forwards'});
 a.onfinish=()=>el.remove()}
const vsCpu=()=>M.mode==='ai'||(M.mode==='online'&&M.kind==='bot');
function oppInfo(){const ai=arenaIdx(REC.tr);
 if(M.mode==='watch'){const p=M.players[1];return{name:p.n,lv:p.lv,tr:p.tr,cpu:false,sub:''}}
 if(M.mode==='online'&&M.kind==='bot')return{name:'Computer',lv:null,tr:null,cpu:true,sub:`${AI_TIER[(M.opp&&M.opp.tier)||0]} bot`};
 if(M.mode==='ai')return{name:'Computer',lv:null,tr:null,cpu:true,sub:`${AI_TIER[M.tier??ai]} bot`};
 const o=M.opp||{};const h=M.oppKey&&REC.opp[M.oppKey];return{name:o.nick||'Opponent',lv:o.lv||null,tr:o.tr??null,cpu:false,sub:`${o.tr!=null?TROPHY+(Number(o.tr)||0):''}${h?` · You ${h.w}–${h.l}`:''}`}}
const meInfo=()=>M.mode==='watch'?{name:M.players[0].n,lv:M.players[0].lv}:({name:nick(),lv:lvlInfo(REC.xp).lv});
function setScore(id,v){const el=$(id);if(el.textContent!==String(v)){el.textContent=v;el.classList.remove('bump');void el.offsetWidth;el.classList.add('bump')}}
function renderGame(){
 if(!V)return;
 const prev=snap();
 const newGame=V.gid!==prevGid;const ev=!newGame&&V.la&&V.la.seq>prevSeq?V.la:null;
 const myT=V.mt&&!V.ov;
 if(ev&&(ev.k==='eye'||ev.k==='sab'))eyeUntil=Date.now()+15000;
 const opp=oppInfo(),me_=meInfo();
 if(newGame){M.oppLast=null;M.placed=null;M.drawn=null;M.ruWas=false}
 if(ev&&(ev.k==='play'||ev.k==='disc'))M.placed=ev.id;
 if(ev&&ev.by==='opp'){if(!M.oppLast||M.lastBy!=='opp')M.oppLast={};const L=M.oppLast;
  if(ev.k==='play'||ev.k==='disc')L.act=ev;else if(ev.k==='draw')L.draw=ev;else if(ev.k==='sab')L.sab=ev;else if(ev.k==='eye')L.eye=ev;else if(ev.k==='rush')L.rush=true}
 if(ev)M.lastBy=ev.by;
 $('oppAv').innerHTML=avatarHTML(opp.name,opp.lv,opp.cpu);$('oppName').textContent=opp.name;$('oppSub').innerHTML=opp.cpu?opp.sub:'';
 $('meAv').innerHTML=avatarHTML(me_.name,me_.lv);$('meName').textContent=me_.name;$('meSub').innerHTML='';
 paintOppHand();paintWatchers();
 setScore('oppScore',total(V.ex[1]));setScore('meScore',total(V.ex[0]));
 const pwb=i=>{const p=V.pw||{sab:[0,0],rush:[0,0]};return `<span class="pwb ${p.sab[i]?'':'used'}" title="Sabotage">SABOTAGE</span>${p.eye?`<span class="pwb ${p.eye[i]?'':'used'}" title="Evil Eye">EVIL EYE</span>`:''}${i===0&&p.rush[0]&&M.mode!=='watch'?`<span class="pwb" title="Rush">RUSH</span>`:''}${V.rn===i?'<span class="pwb hit">RUSHED</span>':''}`};
 $('mePw').innerHTML=pwb(0);$('oppPw').innerHTML=pwb(1);
 $('oppBar').classList.toggle('turn',!V.mt&&!V.ov);$('meBar').classList.toggle('turn',myT);
 $('deckN').textContent=V.dn;$('deck').classList.toggle('low',V.dn<=6);
 let b='';
 for(let c=0;c<5;c++){const C=COL[c],pile=V.di[c],eo=V.ex[1][c],em=V.ex[0][c],so=expScore(eo),sm=expScore(em);
  b+=`<div class="col" data-c="${c}" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct}">
   <div class="stack opp">${eo.map(id=>cardHTML(id)).join('')}</div>
   <div class="cs ols ${so>0?'pos':so<0?'neg':''}">${eo.length?so:''}</div>
   <div class="pile" data-pile="${c}" role="button" tabindex="0" aria-label="${C.n} discard pile">${pile.length?pile.slice(-3).map((id,i,a)=>cardHTML(id,'',`transform:translate(${(a.length-1-i)*-2}px,${(a.length-1-i)*-2}px)`)).join(''):`<span class="ei">${icon(c)}</span>`}${pile.length>1?`<span class="cnt">${pile.length}</span>`:''}</div>
   <div class="cs ols ${sm>0?'pos':sm<0?'neg':''}">${em.length?sm:''}</div>
   <div class="stack me" data-exp="${c}">${em.map(id=>cardHTML(id)).join('')}</div></div>`}
 $('board').innerHTML=b;
 if(M.placed!=null&&!V.ov){const el=document.querySelector(`#board [data-id="${M.placed}"]`);if(el){el.classList.add('last');if(ev&&!reduced)el.classList.add('fresh')}}
 const hand=V.me.slice().sort((a,b)=>cC(a)-cC(b)||cV(a)-cV(b));
 if(sel!=null&&!hand.includes(sel))sel=null;
 if(M.mode==='watch'){$('hand').innerHTML=`<div class="whand bk-${M.players[0].back}" aria-label="${esc(M.players[0].n)} holds ${V.mn||0} cards">${'<div class="back"></div>'.repeat(V.mn||0)}</div>`}
 else $('hand').innerHTML=hand.map(id=>cardHTML(id,sel===id?'sel':'').replace('<div ','<div role="button" tabindex="0" data-h="1" ')).join('');
 if(ev&&ev.by==='me'){if(ev.k==='draw'){const nw=hand.filter(id=>!prev.has(String(id)));M.drawn=ev.p>=0&&hand.includes(ev.id)?ev.id:(nw.length?nw[0]:null)}else if(ev.k==='play'||ev.k==='disc')M.drawn=null}
 if(M.drawn!=null&&!hand.includes(M.drawn))M.drawn=null;
 if(M.drawn!=null&&!V.ov){const el=document.querySelector(`#hand [data-id="${M.drawn}"]`);if(el){el.classList.add('drawn');if(!el.querySelector('.newtag'))el.insertAdjacentHTML('beforeend','<span class="newtag">NEW</span>');if(ev&&ev.k==='draw'&&!reduced)el.classList.add('freshg')}}
 paintEyeMarks();
 $('hand').classList.toggle('off',M.mode==='watch'||!(myT&&V.ph==='play'));
 renderActs();
 let m='';
 if(M.mode==='watch')m=V.ov?watchOverText():`<b>${esc(V.mt?me_.name:opp.name)}</b>'s turn · ${V.ph==='play'?'play or discard':'draw'}`;
 else if(V.ov)m=V.fl==='me'?(V.why==='forfeit'?'<b>You left the battle.</b>':'<b>Your time ran out.</b>'):V.fl==='opp'?(V.why==='forfeit'?`<b>${esc(opp.name)} left the battle.</b>`:`<b>${esc(opp.name)} ran out of time.</b>`):'<b>Battle over!</b>';
 else if(M.mode==='online'&&M.reconnecting)m='Connection lost. Reconnecting…';
 else if(M.mode!=='ai'&&!M.oppHere)m=`${esc(opp.name)} disconnected. Waiting for them to return…`;
 else if(myT)m=V.ph==='play'?(lastText()?`<b>Your turn</b> · ${lastText()}`:'<b>Your turn</b> · play or discard'):'<b>Draw</b> · deck or a discard pile';
 else m=M.mode==='ai'?'Computer is thinking…':`${esc(opp.name)}'s turn…`;
 if(!V.ov&&Date.now()>=eyeUntil)
 $('msg').className='msg';$('msg').innerHTML=m;

 const deckR=rectOf($('deck')),oppR=rectOf($('oppHand'));
 const now=new Set();
 if(newGame){PS=null;closeEmo();$('over').hidden=true;
  if(M.skipVS){M.skipVS=false;dealAnim()}else vsSplash(dealAnim)}
 else document.querySelectorAll('#game [data-id]').forEach(el=>{const id=el.dataset.id;now.add(id);const p=prev.get(id);
  if(p)animateFrom(el,p.r);else if(ev)animateFrom(el,el.closest('#hand')?deckR:oppR)});
 if(!newGame){prev.forEach((p,id)=>{if(!now.has(id))ghost(p.h,p.r,oppR,true)});
  if(ev&&ev.k==='sab'&&ev.by==='me')ghost(`<div class="back bk-${oppBack()}" style="border-radius:8px"></div>`,deckR,oppR,true);
  if(ev&&ev.by==='opp'&&ev.k==='draw'&&ev.p<0)ghost(`<div class="back bk-${oppBack()}" style="border-radius:8px"></div>`,deckR,oppR,true)}
 if(ev&&(ev.k==='eye'||ev.k==='sab')){if(M.mode==='watch'){eyeUntil=0;const a=ev.by==='me'?me_.name:opp.name,b=ev.by==='me'?opp.name:me_.name;
  if(ev.k==='eye'){callout('Evil Eye!','#f6d27c',`${a} looks at one of ${b}'s cards`);sfx('levelup')}else{callout('Sabotage!','#c58bff',`${a} knocks ${b}'s ${COL[cC(ev.id)].n} ${cV(ev.id)||'wager'} away`);shake(2)}}
  else powerScene(ev,opp)}
 if(M.mode==='watch'){const r=V.ru&&!V.ov?V.ru.seat:null;if(r!=null&&M.ruWas!==r){callout('Rushed!','#ff7aa8',`${r===0?me_.name:opp.name} has 10 seconds`);sfx('alarm')}M.ruWas=r}
 else{const nowRu=!!(V.ru&&V.ru.seat===0);if(nowRu&&!M.ruWas&&!V.ov){callout('Rushed!','#ff7aa8','10 seconds. Go!');sfx('alarm');shake(1)}M.ruWas=nowRu}
 if(ev){eventSfx(ev);fxEvent(ev,opp);if(ev.by==='opp'&&ev.k==='draw'&&!V.ov)setTimeout(()=>sfx('turn'),250)}
 scorePops();
 prevGid=V.gid;prevSeq=V.seq;
 if(V.ov)setTimeout(M.mode==='watch'?watchResult:showResult,ev?1800:200);
}
function paintWatchers(){const el=$('watchN');if(!el)return;const n=M.mode==='online'?(M.watchers||0):0;el.hidden=!n;el.innerHTML=n?`${EYEMINI}${n}`:'';el.title=n?`${n} watching`:''}
function watchOverText(){const a=total(V.ex[0]),b=total(V.ex[1]),A=M.players[0].n,B=M.players[1].n;
 if(V.fl)return`<b>${esc(V.fl==='me'?B:A)} wins</b> · ${esc(V.fl==='me'?A:B)} ${V.why==='forfeit'?'left':'ran out of time'}`;
 return a===b?'<b>A draw!</b>':`<b>${esc(a>b?A:B)} wins</b> ${Math.max(a,b)}–${Math.min(a,b)}`}
function watchResult(){if(M.mode!=='watch'||!V||!V.ov||!$('over').hidden&&M.shownFor===V.gid)return;M.shownFor=V.gid;
 const a=total(V.ex[0]),b=total(V.ex[1]),A=M.players[0],B=M.players[1];sfx('win');
 $('over').innerHTML=`<div class="res w"><div class="rib ol">Full time</div><div class="rsub">${watchOverText()}</div>
  <div class="rscore"><div class="rs-p ${a>b||V.fl==='opp'?'win':''}">${avatarHTML(A.n,A.lv)}<b class="ol">${a}</b><span>${esc(A.n)}</span></div><div class="rs-vs">vs</div><div class="rs-p ${b>a||V.fl==='me'?'win':''}">${avatarHTML(B.n,B.lv)}<b class="ol">${b}</b><span>${esc(B.n)}</span></div></div>
  <div class="rbtns"><button class="gb green" id="oAgain">Back to cup</button><button class="gb blue" id="oHome">Lobby</button></div><button class="linkbtn" id="oView">See the table</button></div>`;
 $('over').hidden=false;$('oAgain').onclick=()=>{sfx('ui');stopWatching(true)};$('oHome').onclick=()=>{sfx('ui');stopWatching(false)};$('oView').onclick=()=>{$('over').hidden=true}}
function stopWatching(toCup){const code=M.code;if(M.mode==='watch')NET.raw({t:'unwatch'});M={mode:'home'};V=null;prevGid=null;show('home');renderHome();setTab('cups');if(toCup&&code&&CUPS[code])openCup(code)}
function lastText(){const L=M.oppLast;if(!L)return '';const ch=id=>{const C=COL[cC(id)];return `<span class="mchip" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct}">${C.n} ${cV(id)||'wager'}</span>`};const parts=[];
 if(L.sab)parts.push(`hit your ${ch(L.sab.id)}`);if(L.rush)parts.push('Rush');
 if(L.act)parts.push(`${L.act.k==='play'?'played':'discarded'} ${ch(L.act.id)}`);
 if(L.draw)parts.push(L.draw.auto?'timed out':L.draw.p>=0?`took ${ch(L.draw.id)}`:'drew deck');
 return parts.join(' · ')}
function scorePops(){const cur=[0,1].map(s=>V.ex[s].map(expScore)),lens=[0,1].map(s=>V.ex[s].map(e=>e.length));
 if(PS&&PS.g===V.gid)for(let s=0;s<2;s++)for(let c=0;c<5;c++){if(lens[s][c]>PS.l[s][c]){const d=cur[s][c]-PS.v[s][c];if(!d)continue;
   const el=document.querySelectorAll(`#board .col[data-c="${c}"] .cs`)[s?0:1];setTimeout(()=>popup(el,(d>0?'+':'−')+Math.abs(d),d>0?'up':'dn'),reduced?0:430)}}
 PS={g:V.gid,v:cur,l:lens}}
function fxEvent(ev,opp){const side=ev.by==='me'?0:1,D=reduced?0:430,who=side?opp.name:M.mode==='watch'?meInfo().name:'';const nm=id=>COL[cC(id)].n+' '+(cV(id)||'wager');
 if(ev.k==='play'){const c=ev.p,C=COL[c],v=cV(ev.id),e=V.ex[side][c];
  setTimeout(()=>{const el=document.querySelector(`#board .col[data-c="${c}"] .stack.${side?'opp':'me'} [data-id="${ev.id}"]`);
   if(el){const[x,y]=centerOf(el);burst(x,y,[C.cc,C.cd,'#ffffff'],v===0||v>=9?28:14,{sp:v>=9?7.5:5})}if(v>=9)shake(2)},D);
  if(v===0)callout(`Wager ×${e.filter(id=>cV(id)===0).length+1}`,C.cc,who);
  if(e.length===8)callout('Bonus +20','#7dffa0',who);
  if(side===0&&(v>=9||e.length===8))aiSay(1,.3)}
 else if(ev.k==='disc'){setTimeout(()=>{const el=document.querySelector(`#board .pile[data-pile="${ev.p}"]`);if(el){const[x,y]=centerOf(el);burst(x,y,['#c9d6f5','#8ea3d6'],8,{sp:3,up:1})}},D)}
 else if(ev.k==='sab'){if(side)aiSay(3,.45)}
 else if(ev.k==='rush'){if(!side)callout('Rush set','#ff7aa8','They will not see it coming')}
 else if(ev.k==='flag'){callout(V.why==='forfeit'?'Forfeit':'Time!','#ff5d6c')}
 else if(ev.k==='draw'){if(ev.auto)callout('Too slow!','#ff7aa8',side?`${opp.name} lost a card`:'A random card was discarded');
  if(ev.p>=0)setTimeout(()=>{const el=side?$('oppHand'):document.querySelector(`#hand [data-id="${ev.id}"]`);if(el){const[x,y]=centerOf(el);burst(x,y,[COL[ev.p].cc,'#fff'],10,{sp:4})}},D);
  if(V.ov)callout('Final card!','#ffd23a')}}
function dealAnim(){const deckR=rectOf($('deck'));let i=0;document.querySelectorAll('#hand .card').forEach(el=>animateFrom(el,deckR,i++*70));
 document.querySelectorAll('#oppHand .back').forEach((el,j)=>animateFrom(el,deckR,j*70));sfx('deal');
 setTimeout(()=>{if(!V||V.ov)return;callout(V.mt?'Your turn!':(M.mode==='ai'?'Computer starts':`${oppInfo().name} starts`),V.mt?'#7dffa0':'#ffd23a');if(V.mt)sfx('go');aiSay(0,.5)},650)}
function vsSplash(cb){const o=$('vs'),opp=oppInfo(),me_=meInfo(),ai=arenaIdx(REC.tr);
 const tag=M.mode==='ai'?`${ARENAS[M.tier??ai].n} · ${AI_TIER[M.tier??ai]} computer`:M.kind==='cup'?`${esc(CUPS[M.code]?CUPS[M.code].name:'Cup')} · Cup match`:M.kind==='room'?`Room ${M.code||''} · Friendly battle`:M.kind==='bot'?'Computer battle · Unranked':'Quick match · Ranked';
 o.innerHTML=`<div class="vs-in">
  <div class="vs-band top"><div class="vs-pl">${avatarHTML(opp.name,opp.lv,opp.cpu)}<div><div class="vs-n ol">${esc(opp.name)}</div><div class="vs-t ol">${opp.cpu?esc(opp.sub):TROPHY+(Number(opp.tr)||0)}</div></div></div></div>
  <div class="vs-mid ol">VS</div>
  <div class="vs-band bot"><div class="vs-pl">${avatarHTML(me_.name,me_.lv)}<div><div class="vs-n ol">${esc(me_.name)}</div><div class="vs-t ol">${TROPHY}${REC.tr}</div></div></div></div>
  <div class="vs-ar ol">${esc(tag)}</div></div>`;
 o.hidden=false;o.classList.remove('out');sfx('swoosh');setTimeout(()=>sfx('vs'),380);
 setTimeout(()=>{if(!reduced)burst(innerWidth/2,innerHeight/2,['#ffd23a','#fff','#ffaa00'],30,{sp:9})},520);
 setTimeout(()=>{o.classList.add('out');setTimeout(()=>{o.hidden=true;o.classList.remove('out');cb&&cb()},300)},reduced?600:2100)}
let eyeUntil=0,eyeT=null;
function paintOppHand(){const el=$('oppHand');if(!el||!V)return;setBk(el,oppBack());const busy=Date.now()<eyeUntil;
 if(V.ey>=0&&!busy&&V.on>0)el.innerHTML='<div class="back"></div>'.repeat(V.on-1)+`<div class="peek" title="You saw this card with the Evil Eye">${mini(V.ey,30)}</div>`;
 else el.innerHTML='<div class="back"></div>'.repeat(V.on)}
function paintEyeMarks(){if(!V||Date.now()<eyeUntil)return;document.querySelectorAll('#hand .seen').forEach(e=>e.classList.remove('seen'));
 if(V.em>=0){const el=document.querySelector(`#hand [data-id="${V.em}"]`);if(el){el.classList.add('seen');if(!el.querySelector('.eyetag'))el.insertAdjacentHTML('beforeend','<span class="eyetag">SEEN</span>')}}}
const EYE_SVG='<svg viewBox="0 0 160 100" class="or-eye" aria-hidden="true"><defs><radialGradient id="orIr" cx="50%" cy="45%" r="55%"><stop offset="0" stop-color="#fff4c4"/><stop offset=".22" stop-color="#5fe0d2"/><stop offset=".6" stop-color="#178a9e"/><stop offset="1" stop-color="#0f2f6b"/></radialGradient><linearGradient id="orGd" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#fff3c0"/><stop offset=".5" stop-color="#f2c14e"/><stop offset="1" stop-color="#b8862b"/></linearGradient><radialGradient id="orSc" cx="50%" cy="40%" r="60%"><stop offset="0" stop-color="#fffdf6"/><stop offset="1" stop-color="#ead9f2"/></radialGradient><clipPath id="orClip"><path d="M8 50C40 8 120 8 152 50C120 92 40 92 8 50Z"/></clipPath></defs><g class="or-open"><g clip-path="url(#orClip)"><path d="M8 50C40 8 120 8 152 50C120 92 40 92 8 50Z" fill="url(#orSc)"/><g class="ey-look"><circle cx="80" cy="50" r="25" fill="url(#orIr)" stroke="#0f2f6b" stroke-width="1.5"/><g stroke="#e9fff9" stroke-width=".8" opacity=".45"><path d="M89.0 50.0L102.0 50.0"/><path d="M88.7 52.3L101.3 55.7"/><path d="M87.8 54.5L99.1 61.0"/><path d="M86.4 56.4L95.6 65.6"/><path d="M84.5 57.8L91.0 69.1"/><path d="M82.3 58.7L85.7 71.3"/><path d="M80.0 59.0L80.0 72.0"/><path d="M77.7 58.7L74.3 71.3"/><path d="M75.5 57.8L69.0 69.1"/><path d="M73.6 56.4L64.4 65.6"/><path d="M72.2 54.5L60.9 61.0"/><path d="M71.3 52.3L58.7 55.7"/><path d="M71.0 50.0L58.0 50.0"/><path d="M71.3 47.7L58.7 44.3"/><path d="M72.2 45.5L60.9 39.0"/><path d="M73.6 43.6L64.4 34.4"/><path d="M75.5 42.2L69.0 30.9"/><path d="M77.7 41.3L74.3 28.7"/><path d="M80.0 41.0L80.0 28.0"/><path d="M82.3 41.3L85.7 28.7"/><path d="M84.5 42.2L91.0 30.9"/><path d="M86.4 43.6L95.6 34.4"/><path d="M87.8 45.5L99.1 39.0"/><path d="M88.7 47.7L101.3 44.3"/></g><ellipse class="ey-pupil" cx="80" cy="50" rx="7.5" ry="14" fill="#121a44"/><path d="M70 37l1.5 4.5 4.5 1.5-4.5 1.5-1.5 4.5-1.5-4.5-4.5-1.5 4.5-1.5z" fill="#fff"/></g></g><path d="M8 50C40 8 120 8 152 50C120 92 40 92 8 50Z" fill="none" stroke="url(#orGd)" stroke-width="3.2"/></g><g class="or-lash" stroke="url(#orGd)" stroke-width="2.2" stroke-linecap="round" fill="none"><path d="M29.4 31.4L23.2 28.1"/><path d="M41.0 25.8L35.6 21.3"/><path d="M53.5 21.7L49.4 16.1"/><path d="M66.6 19.3L63.7 10.8"/><path d="M80.0 18.5L80.0 9.5"/><path d="M93.4 19.3L96.3 10.8"/><path d="M106.5 21.7L110.6 16.1"/><path d="M119.0 25.8L124.4 21.3"/><path d="M130.6 31.4L136.8 28.1"/></g></svg>';
const ORB_SVG='<svg viewBox="-100 -100 200 200" class="or-ring" aria-hidden="true"><g fill="none" stroke="#f6d27c"><g class="or-r1"><circle r="95" stroke-width=".7" opacity=".7"/><circle r="88" stroke-width="1.2"/><g stroke-width=".9"><path d="M88.00 0.00L80.00 0.00"/><path d="M87.52 9.20L83.54 8.78"/><path d="M86.08 18.30L82.16 17.46"/><path d="M83.69 27.19L79.89 25.96"/><path d="M80.39 35.79L76.74 34.17"/><path d="M76.21 44.00L69.28 40.00"/><path d="M71.19 51.73L67.96 49.37"/><path d="M65.40 58.88L62.42 56.21"/><path d="M58.88 65.40L56.21 62.42"/><path d="M51.73 71.19L49.37 67.96"/><path d="M44.00 76.21L40.00 69.28"/><path d="M35.79 80.39L34.17 76.74"/><path d="M27.19 83.69L25.96 79.89"/><path d="M18.30 86.08L17.46 82.16"/><path d="M9.20 87.52L8.78 83.54"/><path d="M0.00 88.00L0.00 80.00"/><path d="M-9.20 87.52L-8.78 83.54"/><path d="M-18.30 86.08L-17.46 82.16"/><path d="M-27.19 83.69L-25.96 79.89"/><path d="M-35.79 80.39L-34.17 76.74"/><path d="M-44.00 76.21L-40.00 69.28"/><path d="M-51.73 71.19L-49.37 67.96"/><path d="M-58.88 65.40L-56.21 62.42"/><path d="M-65.40 58.88L-62.42 56.21"/><path d="M-71.19 51.73L-67.96 49.37"/><path d="M-76.21 44.00L-69.28 40.00"/><path d="M-80.39 35.79L-76.74 34.17"/><path d="M-83.69 27.19L-79.89 25.96"/><path d="M-86.08 18.30L-82.16 17.46"/><path d="M-87.52 9.20L-83.54 8.78"/><path d="M-88.00 0.00L-80.00 0.00"/><path d="M-87.52 -9.20L-83.54 -8.78"/><path d="M-86.08 -18.30L-82.16 -17.46"/><path d="M-83.69 -27.19L-79.89 -25.96"/><path d="M-80.39 -35.79L-76.74 -34.17"/><path d="M-76.21 -44.00L-69.28 -40.00"/><path d="M-71.19 -51.73L-67.96 -49.37"/><path d="M-65.40 -58.88L-62.42 -56.21"/><path d="M-58.88 -65.40L-56.21 -62.42"/><path d="M-51.73 -71.19L-49.37 -67.96"/><path d="M-44.00 -76.21L-40.00 -69.28"/><path d="M-35.79 -80.39L-34.17 -76.74"/><path d="M-27.19 -83.69L-25.96 -79.89"/><path d="M-18.30 -86.08L-17.46 -82.16"/><path d="M-9.20 -87.52L-8.78 -83.54"/><path d="M-0.00 -88.00L-0.00 -80.00"/><path d="M9.20 -87.52L8.78 -83.54"/><path d="M18.30 -86.08L17.46 -82.16"/><path d="M27.19 -83.69L25.96 -79.89"/><path d="M35.79 -80.39L34.17 -76.74"/><path d="M44.00 -76.21L40.00 -69.28"/><path d="M51.73 -71.19L49.37 -67.96"/><path d="M58.88 -65.40L56.21 -62.42"/><path d="M65.40 -58.88L62.42 -56.21"/><path d="M71.19 -51.73L67.96 -49.37"/><path d="M76.21 -44.00L69.28 -40.00"/><path d="M80.39 -35.79L76.74 -34.17"/><path d="M83.69 -27.19L79.89 -25.96"/><path d="M86.08 -18.30L82.16 -17.46"/><path d="M87.52 -9.20L83.54 -8.78"/></g><g fill="#f6d27c" stroke="none"><path d="M74.0 -3.0l3 3-3 3-3-3z"/><path d="M64.1 34.0l3 3-3 3-3-3z"/><path d="M37.0 61.1l3 3-3 3-3-3z"/><path d="M0.0 71.0l3 3-3 3-3-3z"/><path d="M-37.0 61.1l3 3-3 3-3-3z"/><path d="M-64.1 34.0l3 3-3 3-3-3z"/><path d="M-74.0 -3.0l3 3-3 3-3-3z"/><path d="M-64.1 -40.0l3 3-3 3-3-3z"/><path d="M-37.0 -67.1l3 3-3 3-3-3z"/><path d="M-0.0 -77.0l3 3-3 3-3-3z"/><path d="M37.0 -67.1l3 3-3 3-3-3z"/><path d="M64.1 -40.0l3 3-3 3-3-3z"/></g></g><g class="or-r2"><circle r="66" stroke-width=".8" stroke-dasharray="2 5"/><circle r="50" stroke-width=".6" opacity=".6"/><g fill="#fff3c0" stroke="none"><circle cx="53.6" cy="22.2" r="2.2"/><circle cx="22.2" cy="53.6" r="2.2"/><circle cx="-22.2" cy="53.6" r="2.2"/><circle cx="-53.6" cy="22.2" r="2.2"/><circle cx="-53.6" cy="-22.2" r="2.2"/><circle cx="-22.2" cy="-53.6" r="2.2"/><circle cx="22.2" cy="-53.6" r="2.2"/><circle cx="53.6" cy="-22.2" r="2.2"/></g><path d="M0-66L0 66M-66 0L66 0" stroke-width=".5" opacity=".45"/></g></g></svg>';
const BOLT_SVG='<svg viewBox="0 0 100 120" class="sb-svg" aria-hidden="true"><defs><linearGradient id="sbG" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff"/><stop offset=".45" stop-color="#d9a6ff"/><stop offset="1" stop-color="#7a2bff"/></linearGradient></defs><path d="M58 4 18 66h26l-8 50 46-68H50z" fill="url(#sbG)" stroke="#2a0a52" stroke-width="3" stroke-linejoin="round"/></svg>';
function powerScene(ev,opp){const o=$('eye');if(!o)return;const sab=ev.k==='sab',mine=ev.by==='me',D=15000;eyeUntil=Date.now()+D;const t0=Date.now();
 clearTimeout(eyeT);const n=Math.max(2,Math.min(9,mine?V.on:V.me.length)),pick=Math.floor(Math.random()*n),C=COL[cC(ev.id)],nm=C.n+' '+(cV(ev.id)||'wager');
 const turn=sab?0:((V.el&&V.el[mine?0:1])||[0,0])[1];
 const face=sab?`<div class="sb-h sb-h1">${mini(ev.id,44)}</div><div class="sb-h sb-h2">${mini(ev.id,44)}</div>`:mini(ev.id,44);
 let fan='';for(let i=0;i<n;i++){const d=i-(n-1)/2;const st=`--r:${(d*7).toFixed(1)}deg;--y:${(Math.abs(d)*Math.abs(d)*1.6).toFixed(1)}px;--x:${(d*26).toFixed(0)}px;--i:${i}`;
  fan+=i===pick?`<div class="ey-c pick" style="${st}"><div class="ey-flip"><div class="ey-bk back"></div><div class="ey-fc">${face}</div></div></div>`:`<div class="ey-c" style="${st}"><div class="back ey-bk"></div></div>`}
 const title=sab?(mine?'SABOTAGE':'SABOTAGED!'):'EVIL EYE';
 const sub=sab?(mine?`Striking ${esc(opp.name)}'s hand…`:`${esc(opp.name)} is striking your hand…`):(mine?`Your gaze falls on ${esc(opp.name)}'s hand`:`${esc(opp.name)} glimpses one of your cards`);
 const cap=sab?(mine?`${esc(opp.name)} draws a new card`:'You draw a replacement'):(mine?`${esc(opp.name)} holds it · your turn ${turn}`:`Exposed to ${esc(opp.name)} · their turn ${turn}`);
 const centre=sab?`<div class="ey-eyew sb-bolt">${BOLT_SVG}</div>`:`<div class="or-stage"><div class="or-halo"></div>${ORB_SVG}<div class="or-eyew">${EYE_SVG}</div></div>`;
 const motes=sab?'':`<div class="or-motes">${Array.from({length:22},(_,k)=>`<i style="left:${(Math.random()*100).toFixed(1)}%;--d:${(Math.random()*2.4).toFixed(2)}s;--s:${(.5+Math.random()).toFixed(2)};--t:${(3.8+Math.random()*2.4).toFixed(2)}s"></i>`).join('')}</div>`;
 o.className='eye '+(sab?'sab ':'oracle ')+(mine?'mine':'theirs')+(reduced?' calm':'');setBk(o,mine?oppBack():EQ.back);
 o.innerHTML=`<div class="ey-veil"></div><div class="ey-crack"></div>${motes}<div class="ey-top ${sab?'ol':''}">${title}</div>${sab?'':'<div class="or-rule"><i></i><b></b><i></i></div>'}${centre}<div class="ey-sub ${sab?'ol':''}">${sub}</div>
  <div class="ey-fan">${fan}<div class="ey-beam"></div><div class="sb-slash"></div><div class="sb-slash s2"></div></div>
  <div class="ey-cap ${sab?'ol':''}"><b class="${sab?'':'ol'}">${esc(nm)}</b>${cap}</div><div class="ey-flash"></div>`;
 o.hidden=false;sfx(sab?'sabx':'eye');
 if(!reduced){if(sab)setTimeout(()=>shake(3),3320);setTimeout(()=>burst(innerWidth/2,innerHeight*.62,sab?['#c58bff','#7a2bff','#fff']:['#ffe9a8','#f6c453','#fff','#c9b6ff'],sab?34:30,{sp:sab?7:5}),3350);if(sab){setTimeout(()=>shake(2),2140);setTimeout(()=>shake(2),2590);setTimeout(()=>burst(innerWidth/2,innerHeight*.5,['#d9a6ff','#fff','#7a2bff'],30,{sp:9}),3825)}}
 // The reveal lands at about 3.3 s and then holds until a tap (or 15 s, since a turn clock may be running).
 let closing=false;const close=()=>{if(closing)return;closing=true;clearTimeout(eyeT);o.onclick=null;o.classList.add('out');setTimeout(()=>{o.hidden=true;o.classList.remove('out');eyeUntil=0;if(V)renderGame()},600)};
 o.onclick=()=>{if(Date.now()-t0>=(reduced?800:3750))close()};
 eyeT=setTimeout(close,D-600)}
function renderActs(){const myT=V&&V.mt&&!V.ov&&M.mode!=='watch';let h='';$('emoBtn').hidden=M.mode==='watch';
 if(M.mode==='watch'){$('acts').innerHTML=V&&V.ov?'':'<span class="watchnote">You are watching. Hands stay hidden.</span>';return}
 if(myT&&V.ph==='play'&&sel!=null&&!pending)h=`<button class="gb green" id="aPlay">Play to ${COL[cC(sel)].n}</button><button class="gb gray" id="aDisc">Discard</button>`;
 else if(V&&V.ov)h=`<button class="gb" id="aAgain">${M.mode==='ai'?'Battle again':'Rematch'}</button>`;
 else if(myT&&!pending){const p=V.pw||{sab:[0,0],rush:[0,0]};
  if(V.ph==='play'&&p.sab[0]>0&&V.dn>=3)h+=`<button class="gb purple ${armed==='sab'?'armed':''}" id="aSab">${armed==='sab'?'Tap to confirm':'Sabotage'}</button>`;
  if(V.ph==='play'&&p.eye&&p.eye[0]>0&&V.dn>=10&&!(V.ru&&V.ru.seat===0))h+=`<button class="gb purple eyebtn ${armed==='eye'?'armed':''}" id="aEye">${armed==='eye'?'Tap to confirm':'Evil Eye'}</button>`;
  if(p.rush[0]>0&&V.rn==null)h+=`<button class="gb purple ${armed==='rush'?'armed':''}" id="aRush">${armed==='rush'?'Tap to confirm':'Rush'}</button>`}
 $('acts').innerHTML=h}
let armed=null,armT=null;
function arm(k){armed=k;clearTimeout(armT);armT=setTimeout(()=>{armed=null;renderActs()},3000);sfx('select');renderActs()}
function setSel(id){sel=sel===id?null:id;document.querySelectorAll('#hand .card').forEach(el=>el.classList.toggle('sel',+el.dataset.id===sel));renderActs();if(sel!=null)sfx('select')}
function err(text,el){sfx('bad');$('msg').className='msg err';$('msg').textContent=text;if(el){el.classList.remove('shake');void el.offsetWidth;el.classList.add('shake')}}

/* ================= Emotes ================= */
let emoCool=0,emoMute=false,lastAiEmo=0;
const bubT={};
function showBubble(side,text){if(side==='opp'&&emoMute)return;const el=$(side==='me'?'meBub':'oppBub');el.textContent=text;el.hidden=false;el.style.animation='none';void el.offsetWidth;el.style.animation='';
 clearTimeout(bubT[side]);bubT[side]=setTimeout(()=>{el.hidden=true},2600);sfx('emote')}
function renderEmo(){$('emoPn').innerHTML=EMOTES.map((t,i)=>`<button class="e" data-emo="${i}">${t}</button>`).join('')+`<button class="mute" id="emoMute">${emoMute?'Show rival emotes':'Mute rival emotes'}</button>`}
function openEmo(){renderEmo();$('emoPn').hidden=false;$('emoBtn').setAttribute('aria-expanded','true');sfx('ui')}
function closeEmo(){$('emoPn').hidden=true;$('emoBtn').setAttribute('aria-expanded','false')}
function sendEmote(i){if(Date.now()<emoCool)return;emoCool=Date.now()+2500;closeEmo();showBubble('me',EMOTES[i]);const e={k:i,t:Date.now()};
 if(M.mode==='online')NET.raw({t:'emote',k:e.k});
 else if(M.mode==='ai'){if(i===0)aiSay(0,.8,true);else if(i===5)aiSay(5,.85,true);else if(i===4)aiSay(3,.3,true)}}
function aiSay(i,p,force){if(M.mode!=='ai'||Math.random()>p)return;if(!force&&Date.now()-lastAiEmo<6000)return;lastAiEmo=Date.now();setTimeout(()=>{if(M.mode==='ai')showBubble('opp',EMOTES[i])},700+Math.random()*500)}
function oppEmote(e){if(!e||typeof e.k!=='number'||!EMOTES[e.k])return;if(M.oppEmoT===undefined){M.oppEmoT=e.t;return}if(e.t!==M.oppEmoT){M.oppEmoT=e.t;showBubble('opp',EMOTES[e.k])}}

/* ================= Results ================= */
let shownFor=null;
function outcome(){const me=total(V.ex[0]),them=total(V.ex[1]);return{me,them,r:V.fl?(V.fl==='me'?'l':'w'):me>them?'w':me<them?'l':'d'}}
function finalize(){if(!V||!V.ov)return null;const{me,them,r}=outcome(),o=oppInfo();
 return recordGame(vsCpu()?'ai':'pvp',o.name,vsCpu()?'ai':(M.oppKey||'n:'+o.name),me,them,V.gid,r,M.kind==='cup')}
function showResult(){if(!V||!V.ov||M.mode==='home')return;if(shownFor===V.gid&&!$('over').hidden)return;
 const first=shownFor!==V.gid;shownFor=V.gid;const info=finalize();const{me,them,r}=outcome(),o=oppInfo();
 if(first){sfx(r==='l'?'lose':'win');if(r==='w')confetti();if(M.mode==='ai')setTimeout(()=>aiSay(5,.75,true),900)}
 const title=r==='w'?'Victory!':r==='l'?'Defeat':'Draw';
 const reason=V.fl?(V.why==='forfeit'?(V.fl==='me'?'You left the battle':`${o.name} left the battle`):(V.fl==='me'?'Your clock ran out':`${o.name}'s clock ran out`)):r==='d'?'Dead even':`${Math.abs(me-them)} point ${r==='w'?'win':'loss'}`;
 const lanes=COL.map((C,c)=>`<div class="rc" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct}"><b>${V.ex[0][c].length?expScore(V.ex[0][c]):'–'}</b><span>${V.ex[1][c].length?expScore(V.ex[1][c]):'–'}</span></div>`).join('');
 const h=!vsCpu()&&M.oppKey&&REC.opp[M.oppKey];
 const L0=lvlInfo(info?info.x0:REC.xp);
 $('over').innerHTML=`<div class="res ${r}">
  <div class="rib ol">${title}</div><div class="rsub">${esc(reason)}</div>
  <div class="rscore"><div class="rs-p ${r==='w'?'win':''}">${avatarHTML(nick(),lvlInfo(REC.xp).lv)}<b class="ol">${me}</b><span>You</span></div><div class="rs-vs">vs</div><div class="rs-p ${r==='l'?'win':''}">${avatarHTML(o.name,o.lv,o.cpu)}<b class="ol">${them}</b><span>${esc(o.name)}</span></div></div>
  <div class="rcols" aria-label="Trail scores, yours on top">${lanes}</div>
  <div class="rrow">${TROPHY}<div class="bar"><i id="rTrBar"></i><b class="ols" id="rTrTxt"></b></div><span class="dchip ${info&&info.dT<0?'neg':info&&info.dT===0?'zero':''}" id="rTrD">${info?(info.dT>0?'+':'')+info.dT:'0'}</span></div>
  <div class="rrow coinrow" id="rCoinRow">${PIGGY}<div class="bar coin"><b class="ols" id="rCoinTxt">${M.kind==='cup'&&M.mode==='online'?'Cup match · prizes when the cup ends':M.mode==='online'?'Counting coins…':'Play online to earn coins'}</b></div><span class="dchip coin" id="rCoinD" ${M.mode==='online'&&M.kind!=='cup'?'':'hidden'}>+0</span></div>
  <div class="rrow"><div class="lvbox ol" id="rLv">${L0.lv}</div><div class="bar xp"><i id="rXpBar"></i><b class="ols" id="rXpTxt"></b></div><span class="dchip xp">+${info?info.dX:0} XP</span></div>
  ${h?`<p class="note" style="text-align:center">Head to head with ${esc(o.name)}: ${wld(h)}</p>`:''}
  <div class="rbtns"><button class="gb green" id="oAgain">${M.mode==='ai'?'Battle again':M.kind==='cup'?'Back to cup':'Rematch'}</button><button class="gb blue" id="oHome">Lobby</button></div>
  <button class="linkbtn" id="oView">See the table</button><p class="note" id="rmNote" style="text-align:center"></p></div>`;
 $('over').hidden=false;
 $('oAgain').onclick=()=>{sfx('ui');again()};$('oHome').onclick=()=>{sfx('ui');goHome()};$('oView').onclick=()=>{$('over').hidden=true};
 updateRmNote();
 if(info)animateGains(info,first);
 if(M.mode==='online'&&M.earned)setTimeout(()=>showEarned(first),first?2600:0)}
/* Icons that fly from one place on screen into a counter, landing one after another. */
function flyIcons(html,fromEl,toEl,n,onHit,done){const a=fromEl&&fromEl.getBoundingClientRect(),b=toEl&&toEl.getBoundingClientRect();
 if(!a||!b||!a.width||!b.width||reduced||n<1||typeof document.body.animate!=='function'){for(let i=0;i<n;i++)onHit&&onHit(i);done&&done();return}
 const tx=b.left+b.width/2,ty=b.top+b.height/2;let left=n;
 for(let i=0;i<n;i++){const d=document.createElement('div');d.className='flyico';d.innerHTML=html;document.body.appendChild(d);
  const sx=a.left+a.width/2+(Math.random()-.5)*a.width*.7,sy=a.top+a.height/2+(Math.random()-.5)*a.height*.5,mx=(sx+tx)/2+(Math.random()-.5)*120,my=Math.min(sy,ty)-60-Math.random()*60;
  const an=d.animate([{transform:`translate(${sx}px,${sy}px) scale(.2)`,opacity:0},{transform:`translate(${sx}px,${sy-24}px) scale(1.15)`,opacity:1,offset:.18},{transform:`translate(${mx}px,${my}px) scale(1)`,opacity:1,offset:.55},{transform:`translate(${tx}px,${ty}px) scale(.55)`,opacity:.9}],{duration:900,delay:i*95,easing:'cubic-bezier(.5,0,.6,1)',fill:'both'});
  an.onfinish=()=>{d.remove();toEl.classList.remove('hit');void toEl.offsetWidth;toEl.classList.add('hit');onHit&&onHit(i);if(--left===0)done&&done()}}}
function showEarned(first){const e=M.earned;if(!e||!$('rCoinRow'))return;const txt=$('rCoinTxt'),chip=$('rCoinD');
 if(e.cup){txt.textContent='Cup match · prizes when the cup ends';chip.hidden=true;return}
 if(!e.counted){txt.textContent='Too short for coins';chip.hidden=true;return}
 if(e.coins<=0){txt.textContent="Today's battle coins are collected";chip.hidden=true;return}
 const total=ME?ME.coins:null;
 if(!first){txt.textContent=total!=null?`${total} coins`:'Coins added';chip.textContent='+'+e.coins;return}
 const base=total!=null?total-e.coins:0;txt.textContent=total!=null?`${base} coins`:'';chip.textContent='+0';
 depositCoins(e.coins,document.querySelector('#over .rib'),base,{pig:$('rCoinRow').querySelector('.pg'),
  set:v=>{chip.textContent='+'+(v-base);txt.textContent=total!=null?`${v} coins`:''},
  done:()=>{chip.classList.remove('pulse');void chip.offsetWidth;chip.classList.add('pulse')}})}
function trState(t){const a=arenaIdx(t),A=ARENAS[a],N=ARENAS[a+1];return{pct:N?(t-A.at)/(N.at-A.at)*100:100,txt:N?`${t} / ${N.at}`:`${t}`}}
function animateGains(info,first){const set=(t,x)=>{const s=trState(Math.round(t));if(!$('rTrBar'))return;$('rTrBar').style.width=s.pct+'%';$('rTrTxt').textContent=s.txt;const L=lvlInfo(Math.round(x));$('rXpBar').style.width=(L.into/L.need*100)+'%';$('rXpTxt').textContent=`${L.into} / ${L.need} XP`;
  if(+$('rLv').textContent!==L.lv){$('rLv').textContent=L.lv;$('rLv').classList.remove('up');void $('rLv').offsetWidth;$('rLv').classList.add('up');if(first){sfx('levelup');callout('Level up!','#7fd0ff',`Level ${L.lv}`)}}};
 if(!first){set(info.t1,info.x1);return}
 set(info.t0,info.x0);let lastT=info.t0;
 if(info.dT>0)setTimeout(()=>flyIcons(TROPHY,document.querySelector('#over .rib'),document.querySelector('#over .rrow>svg'),Math.min(8,Math.max(3,Math.round(info.dT/4))),()=>sfx('coin')),250);
 setTimeout(()=>tween(900,k=>{const t=info.t0+(info.t1-info.t0)*k;if(Math.round(t)!==Math.round(lastT)&&Math.round(t)%3===0)sfx('coin');lastT=t;set(t,info.x0)},()=>{
  tween(1000,k=>set(info.t1,info.x0+(info.x1-info.x0)*k),()=>{
   if(info.a1>info.a0){sfx('arena');confetti();callout('New arena!',ARENAS[info.a1].g[0],ARENAS[info.a1].n);callout('New cards!','#ffd23a',`${CARDSETS[info.a1].n} style unlocked and equipped`)}})}),700)}
function updateRmNote(){const n=$('rmNote');if(!n||!V)return;n.textContent=M.mode==='ai'?'':(M.closed||M.oppLeft)?'Your opponent has left the battle.':M.iWantRm===V.gid?(M.theyWantRm===V.gid?'Starting…':'Waiting for your opponent…'):(M.theyWantRm===V.gid?'Your opponent wants a rematch!':'')}

/* ================= Actions ================= */
function act(kind,arg,x,el){
 if(!V||V.ov||pending||M.mode==='watch')return;armed=null;
 if(kind==='eye'){if(!V.mt||Date.now()<eyeUntil)return;
  if(M.mode==='ai'){if(!okEye(M.S,0))return;doEye(M.S,0);afterLocal()}
  else if(M.mode==='online'){if(NET.send({t:'move',seq:V.seq,a:'eye'}))pending=true;renderActs()}
  return}
 if(kind==='sab'||kind==='rush'){if(!V.mt)return;
  if(M.mode==='ai'){const S=M.S;if(kind==='sab'&&okSab(S,0))doSab(S,0);else if(kind==='rush'&&okRush(S,0))doRush(S,0);else return;afterLocal()}
  else if(M.mode==='online'){if(NET.send({t:'move',seq:V.seq,a:kind}))pending=true;renderActs()}
  return}
 if(kind==='play'){
  if(!(V.mt&&V.ph==='play'))return;const e=V.ex[0][cC(arg)];
  if(x&&!canE(e,arg))return err(whyNot(e,arg),el||document.querySelector(`#hand [data-id="${arg}"]`));
 }else{
  if(!(V.mt&&V.ph==='draw'))return;
  if(arg>=0&&!V.di[arg].length)return err('That pile is empty.',el);
  if(arg>=0&&arg===V.jd)return err('You just discarded there. Draw from somewhere else.',el);
 }
 sel=null;
 if(M.mode==='ai'){const S=M.S;
  if(kind==='play')doPlay(S,0,arg,x);else doDraw(S,0,arg);afterLocal()}
 else if(M.mode==='online'){const mv=kind==='play'?{t:'move',seq:V.seq,a:x?'play':'disc',id:arg}:{t:'move',seq:V.seq,a:'draw',pile:arg};if(NET.send(mv))pending=true;renderActs()}
}
function freshView(S){V=viewFor(S,0);V.at=Date.now()}
function afterLocal(){freshView(M.S);renderGame();if(M.mode==='ai')scheduleAi()}
function scheduleAi(){clearTimeout(aiT);const S=M.S;if(S.over||S.turn!==1)return;
 const alive=()=>M.mode==='ai'&&M.S===S&&!S.over&&S.turn===1;
 const rr=()=>{freshView(S);renderGame()};
 const playStep=()=>{if(!alive())return;if(Date.now()<eyeUntil){aiT=setTimeout(playStep,300);return}const b=aiChoosePlay(S,AI_NOISE[M.tier||0]);doPlay(S,1,b.id,b.x);rr();
  aiT=setTimeout(()=>{if(!alive())return;doDraw(S,1,aiChooseDraw(S));rr()},780)};
 const eyeStep=()=>{if(!alive())return;if(okEye(S,1)&&Math.random()<.22){doEye(S,1);rr();aiT=setTimeout(playStep,6600)}else playStep()};
 const sabStep=()=>{if(!alive())return;const L=S.deck.length;if(okSab(S,1)&&L<=30&&L>=6&&Math.random()<.25){doSab(S,1);rr();aiT=setTimeout(eyeStep,6600)}else eyeStep()};
 const wait=Math.max(850,(S.t0||0)-Date.now()+1300);
 aiT=setTimeout(()=>{if(!alive())return;if(okRush(S,1)&&S.deck.length<=34&&Math.random()<.2){doRush(S,1);rr();aiT=setTimeout(sabStep,1200)}else sabStep()},wait)}
function again(){$('over').hidden=true;
 if(M.mode==='ai'){startAi();return}
 if(M.kind==='cup'){const code=M.code;goHome();setTab('cups');if(code&&CUPS[code])openCup(code);return}
 if(!V||!V.ov)return;
 if(M.closed||M.oppLeft||!NET.ready){toast('Your opponent has left. Find a new match from the lobby.');showResult();return}
 if(NET.send({t:'rematch'}))M.iWantRm=V.gid;
 showResult()}
function askLeave(){if(M.mode==='watch'){stopWatching(true);return}
 if(V&&!V.ov&&(M.mode==='ai'||M.mode==='online')){
  $('leaveMsg').textContent=vsCpu()?'Leaving a battle against the computer is free. It will not count as a win or a loss.':'Leaving now counts as a loss.';openSheet('leaveSheet')}else goHome()}
function forfeit(){closeSheets();if(!V||V.ov){goHome();return}
 if(vsCpu()){goHome();return} // a battle against the computer: nothing is recorded and no trophies change
 const{me,them}=outcome(),o=oppInfo();
 recordGame(M.mode==='ai'?'ai':'pvp',o.name,M.mode==='ai'?'ai':(M.oppKey||'n:'+o.name),me,them,V.gid,'l');
 sfx('lose');goHome()}

/* ================= Mode: vs computer ================= */
function leaveRoom(){clearTimeout(aiT)}
function startAi(){leaveRoom();const tier=arenaIdx(REC.tr);M={mode:'ai',tier,S:deal(Math.random()<.5?0:1,clockMs())};freshView(M.S);$('roomTag').textContent=ARENAS[tier].n;show('game');renderGame();scheduleAi()}

/* ================= Clock ticker ================= */
const fmt=ms=>{if(ms<10000)return (Math.max(0,ms)/1000).toFixed(1);const t=Math.ceil(ms/1000);return Math.floor(t/60)+':'+String(t%60).padStart(2,'0')};
let lastTickSec=null,warned=null;
function tickClock(){
 const ok=V&&!$('game').hidden;const now=Date.now();tickRewards();
 if(M.mode==='room'&&M.expires){const s=Math.ceil(Math.max(0,M.expires-now)/1000);$('waitTimer').textContent=`The room closes in ${Math.floor(s/60)}:${String(s%60).padStart(2,'0')} if nobody joins.`}
 if(ok)[['meClk',0],['oppClk',1]].forEach(([id,i])=>{const el=$(id);const rushOn=V.ru&&V.ru.seat===i&&!V.ov;el.hidden=!(V.ck||rushOn);el.classList.toggle('rush',!!rushOn);
  if(rushOn){const l=Math.max(0,V.ru.l-(V.ru.pz?0:now-V.at));el.textContent=(l/1000).toFixed(1);el.classList.add('run');el.classList.remove('low');
   if(i===0){const sec=Math.ceil(l/1000);if(l>0&&sec!==lastTickSec){lastTickSec=sec;sfx('tick')}}return}
  if(!V.ck)return;let l=V.ck.l[i];const run=V.ck.run===i;if(run)l=Math.max(0,l-Math.max(0,(now-V.at)-(V.ck.d||0)));
  el.textContent=fmt(l);el.classList.toggle('run',run);el.classList.toggle('low',l<=Math.min(30000,V.ck.b*.1));
  if(i===0&&run){const sec=Math.ceil(l/1000);if(l<=10000&&l>0&&sec!==lastTickSec){lastTickSec=sec;sfx('tick')}
   const wk=V.gid+':'+V.seq;if(l<=30000&&l>10000&&warned!==wk&&V.ck.b>60000){warned=wk;sfx('low')}}});
 if(M.mode==='ai'&&M.S&&!M.S.over&&M.S.rushUntil&&M.S.rushRem==null&&now>=M.S.rushUntil){clearTimeout(aiT);autoMove(M.S,M.S.turn);afterLocal();return}
 if(M.mode==='ai'&&M.S&&M.S.clk&&!M.S.over&&!M.S.clk.paused){const S=M.S;
  if(clockLeft(S,S.turn,now)<=0){clearTimeout(aiT);doFlag(S,S.turn,'time');afterLocal()}}
}
setInterval(tickClock,100);

/* ================= Events ================= */
document.addEventListener('click',e=>{
 const t=e.target;
 const cl=t.closest&&t.closest('[data-claim]');if(cl){sfx('ui');NET.send({t:'claim_quest',slot:cl.dataset.claim});return}
 if(t.closest('[data-close]')){sfx('ui');closeSheets();return}
 if(t.classList&&t.classList.contains('sheet')&&t.id!=='intro'&&t.id!=='leaveSheet'){closeSheets();return}
 const em=t.closest('[data-emo]');if(em){sendEmote(+em.dataset.emo);return}
 if(t.closest('#emoMute')){emoMute=!emoMute;renderEmo();return}
 if(!$('emoPn').hidden&&!t.closest('#emoPn')&&!t.closest('#emoBtn'))closeEmo();
 const hc=t.closest('#hand .card');if(hc){if(V&&V.mt&&V.ph==='play'&&!V.ov&&!pending)setSel(+hc.dataset.id);return}
 if(t.closest('#aPlay'))return act('play',sel,true);
 if(t.closest('#aDisc'))return act('play',sel,false);
 if(t.closest('#aAgain')){sfx('ui');return again()}
 if(t.closest('#aSab'))return armed==='sab'?act('sab'):arm('sab');
 if(t.closest('#aEye'))return armed==='eye'?act('eye'):arm('eye');
 if(t.closest('#aRush'))return armed==='rush'?act('rush'):arm('rush');
 const pl=t.closest('.pile');
 if(pl){const c=+pl.dataset.pile;if(V&&V.ph==='play'&&sel!=null&&cC(sel)===c)return act('play',sel,false,pl);return act('draw',c,false,pl)}
 const ex=t.closest('.stack.me');if(ex&&sel!=null&&cC(sel)===+ex.dataset.exp)return act('play',sel,true);
});
document.addEventListener('keydown',e=>{if((e.key==='Enter'||e.key===' ')&&e.target.matches('#hand .card,.pile')){e.preventDefault();e.target.click()}
 if(e.key==='Escape'){closeEmo();if(!$('friendSheet').hidden||!$('setSheet').hidden||!$('leaveSheet').hidden)closeSheets()}});
$('deck').onclick=()=>act('draw',-1,false,$('deck'));
$('playAi').onclick=()=>{sfx('ui');startAi()};
$('openFriend').onclick=()=>{sfx('ui');openSheet('friendSheet')};
const shakeEl=el=>{el.classList.remove('shake');void el.offsetWidth;el.classList.add('shake');el.focus()};
$('create').onclick=()=>{sfx('ui');const raw=$('customCode').value.trim();
 if(raw&&!/^[A-Za-z]{5}$/.test(raw)){sfx('bad');shakeEl($('customCode'));$('roomNote').textContent='Your own code must be exactly 5 letters, or leave it empty for a random one.';return}
 if(NET.ready)sendProfile();if(NET.send({t:'create',clock:clkSet,code:raw?raw.toUpperCase():undefined}))closeSheets()};
$('customCode').addEventListener('input',e=>{e.target.value=e.target.value.toUpperCase().replace(/[^A-Z]/g,'')});
$('customCode').addEventListener('keydown',e=>{if(e.key==='Enter')$('create').click()});
$('quickBtn').onclick=()=>{sfx('ui');if(NET.ready)sendProfile();if(NET.send({t:'queue',clock:clkSet}))closeSheets()};
$('join').onclick=()=>{const c=$('joinCode').value.trim().toUpperCase();if(!/^[A-Z]{5}$/.test(c)){sfx('bad');$('joinCode').classList.remove('shake');void $('joinCode').offsetWidth;$('joinCode').classList.add('shake');$('joinCode').focus();return}sfx('ui');if(NET.ready)sendProfile();if(NET.send({t:'join',code:c}))closeSheets()};
$('joinCode').addEventListener('keydown',e=>{if(e.key==='Enter')$('join').click()});
$('joinCode').addEventListener('input',e=>{e.target.value=e.target.value.toUpperCase().replace(/[^A-Z]/g,'')});
const nickNote=(t,bad)=>{const n=$('nickNote');n.textContent=t||'';n.classList.toggle('bad',!!bad)};
function saveNick(){nickNote('');const was=NAME,b=$('nickSave');trySetName($('nick').value,(ok,msg)=>{
  if(ok){const same=NAME===was&&!msg;nickNote(msg||(same?`That is already your name, ${NAME}.`:`Saved! You are now ${NAME}.`));sfx(same?'ui':'levelup');
   $('nick').value=NAME;b.textContent='Saved ✓';b.classList.add('saved');clearTimeout(b._t);b._t=setTimeout(()=>{b.textContent='Save';b.classList.remove('saved')},1800);
   if(!same){toast(`Name saved: ${NAME}`);renderHome()}}
  else{nickNote(msg,true);sfx('bad');$('nick').focus()}})}
$('nick').value=NAME;$('nickSave').onclick=saveNick;$('nick').addEventListener('keydown',e=>{if(e.key==='Enter')saveNick()});
$('topProf').onclick=$('gearBtn').onclick=()=>{sfx('ui');$('nick').value=NAME;nickNote('');renderCardPick();openSheet('setSheet')};
/* ---- Card style picker (Settings) ---- */
const CS_SAMPLE=[5,7,8,9,10,6,10];
function renderCardPick(){const max=cardsOpen();
 $('cardPick').innerHTML=CARDSETS.map((s,i)=>{const lock=i>max;
  return `<button type="button" class="cs cs-${s.k}${i===cardSel?' on':''}${lock?' lock':''}" data-cs="${i}" aria-pressed="${i===cardSel}" aria-label="${s.n} cards${lock?`, locked until ${ARENAS[i].at} trophies`:''}">${mini(cid(i%5,CS_SAMPLE[i]),40)}<span class="csn">${s.n}</span>${lock?`<span class="csl">${IC.lock}${ARENAS[i].at}</span>`:''}</button>`}).join('')}
$('cardPick').addEventListener('click',e=>{const b=e.target.closest('[data-cs]');if(!b)return;const i=+b.dataset.cs;
 if(i>cardsOpen()){sfx('bad');toast(`Reach ${ARENAS[i].n} (${ARENAS[i].at} trophies) to unlock ${CARDSETS[i].n} cards.`);return}
 sfx('select');cardSel=i;ST.set('ft-cards',i);applyCards();renderCardPick()});
$('replayIntro').onclick=()=>{sfx('ui');openIntro()};
/* ---- Rewards, quests, computer offer ---- */
$('chestBtn').onclick=()=>{sfx('ui');if(ME&&ME.chest.n>0)NET.send({t:'claim_chest'});else if(ME&&ME.chest.nextIn!=null)toast(`Your next free chest arrives in ${fmtSpan(left(ME.chest.nextIn))}.`)};
document.querySelectorAll('#cupFmt [data-fmt]').forEach(b=>b.onclick=()=>{sfx('select');cupFmtSel=b.dataset.fmt;paintCupNew()});
document.querySelectorAll('#cupClk [data-cclk]').forEach(b=>b.onclick=()=>{sfx('select');cupClkSel=+b.dataset.cclk;paintCupNew()});
$('cupMake').onclick=()=>{const name=$('cupName').value.trim();if(!name){sfx('bad');toast('Give the cup a name.');return}
 if(!NET.ready){sfx('bad');toast('Connect to the game server to create a cup.');return}
 sfx('ui');if(NET.send({t:'cup_create',name,format:cupFmtSel,clock:cupClkSel})){closeSheets();setTab('cups');cupAwaitNew=true}};
const cupJoinGo=()=>{const code=$('cupCode').value.trim().toUpperCase();if(!/^[A-Z]{6}$/.test(code)){sfx('bad');$('cupJoinNote').textContent='A cup code is 6 letters.';return}
 if(!NET.ready){sfx('bad');$('cupJoinNote').textContent='Connect to the game server first.';return}
 sfx('ui');cupAwaitJoin=code;NET.send({t:'cup_join',code})};
$('cupJoinGo').onclick=cupJoinGo;$('cupCode').addEventListener('keydown',e=>{if(e.key==='Enter')cupJoinGo()});
$('coinBox').onclick=()=>{sfx('ui');shopArmed=null;renderShop();openSheet('shopSheet')};
$('shopOpen').onclick=()=>{sfx('ui');shopArmed=null;renderShop();openSheet('shopSheet')};
$('questBtn').onclick=()=>{sfx('ui');renderQuests();openSheet('questSheet')};
$('botGo').onclick=()=>{sfx('ui');NET.send({t:'bot'})};
$('botWait').onclick=()=>{sfx('ui');$('botOffer').hidden=true};
$('seasonOk').onclick=()=>{sfx('ui');closeSheets()};
$('remTog').onclick=toggleReminders;
/* ---- Account: recovery code ---- */
let keyShown=false,restoreArmed=null,restoreT=null;
function paintKey(){$('keyShow').textContent=keyShown?fmtKey(KEY):'••••• ••••• ••••• •••••';$('keyReveal').textContent=keyShown?'Hide':'Show'}
const keyNote=(t,bad)=>{const n=$('keyNote');n.textContent=t||'';n.classList.toggle('bad',!!bad)};
$('keyReveal').onclick=()=>{sfx('ui');keyShown=!keyShown;paintKey()};
$('keyCopy').onclick=async()=>{sfx('ui');toast(await copyText(fmtKey(KEY))?'Recovery code copied. Keep it somewhere safe.':'Could not copy. Tap Show and write it down.')};
$('keyIn').addEventListener('input',e=>{e.target.value=e.target.value.toUpperCase().replace(/[^A-Z0-9 -]/g,'')});
$('keyRestore').onclick=()=>{
 const k=cleanKey($('keyIn').value);
 if(!k){sfx('bad');keyNote('A recovery code has 20 letters and numbers, like ABCDE-FGHJK-MNPQR-STUVW.',true);return}
 if(k===KEY){keyNote('That is the code for the account on this device already.');return}
 if(!NET.ready){sfx('bad');keyNote('Connect to the game server first, then try again.',true);return}
 if(restoreArmed!==k){restoreArmed=k;$('keyRestore').textContent='Tap to confirm';sfx('select');
  keyNote(`This replaces the account on this device. Its recovery code is ${fmtKey(KEY)}. Save it first if you want to come back.`);
  clearTimeout(restoreT);restoreT=setTimeout(()=>{restoreArmed=null;$('keyRestore').textContent='Restore'},10000);return}
 restoreArmed=null;clearTimeout(restoreT);$('keyRestore').textContent='Restore';
 RESTORE.prev=KEY;ST.set('ft-restore',KEY);KEY=k;ST.set('ft-key',k);paintKey();keyNote('Restoring…');NET.reconnect()};
document.querySelectorAll('.nav button').forEach(b=>b.onclick=()=>{if(b.getAttribute('aria-selected')!=='true')sfx('ui');setTab(b.dataset.tab)});
$('clkpick').addEventListener('click',e=>{const b=e.target.closest('button');if(!b)return;clkSet=+b.dataset.clk;ST.set('ft-clock',clkSet);paintPicker();sfx('select')});
const goHome=(quiet)=>{leaveRoom();if(!quiet){if(M.mode==='online')NET.raw({t:'leave_match'});else if(M.mode==='queue')NET.raw({t:'unqueue'});else if(M.mode==='room')NET.raw({t:'cancel_room'});else if(M.mode==='cupwait')NET.raw({t:'unqueue'});else if(M.mode==='watch')NET.raw({t:'unwatch'})}
 if(M.mode==='online')SS.set('ft-resume',null);M={mode:'home'};V=null;prevGid=null;sel=null;pending=false;show('home');renderHome()};
$('waitCancel').onclick=()=>{sfx('ui');goHome()};
$('leave').onclick=()=>{sfx('ui');askLeave()};
$('stay').onclick=()=>{sfx('ui');closeSheets()};
$('forfeit').onclick=forfeit;
$('emoBtn').onclick=()=>{$('emoPn').hidden?openEmo():closeEmo()};
function paintSound(){$('sfx').innerHTML=sfxOn?IC.sndOn:IC.sndOff;$('sfx').setAttribute('aria-label',sfxOn?'Mute sound':'Turn sound on');$('sndTog').setAttribute('aria-checked',String(sfxOn));$('musTog').setAttribute('aria-checked',String(musicOn))}
const toggleSound=()=>{sfxOn=!sfxOn;ST.set('ft-sfx',sfxOn);paintSound();if(sfxOn)sfx('ui')};
$('sfx').onclick=toggleSound;$('sndTog').onclick=toggleSound;
$('musTog').onclick=()=>{musicOn=!musicOn;ST.set('ft-music',musicOn);paintSound();musicSync()};

/* ================= Boot ================= */
$('navBattle').innerHTML=IC.swords+'Battle';$('navLeaders').innerHTML=IC.crown+'Ranks';$('navCups').innerHTML=CUPIC+'Cups';$('navRecords').innerHTML=IC.cup+'Records';$('navGuide').innerHTML=IC.book+'Guide';
$('gearBtn').innerHTML=IC.gear;$('leave').innerHTML=IC.x;$('emoBtn').innerHTML=IC.chat;$('clkLbl').innerHTML=IC.clock+'Clock each';
document.querySelectorAll('.xbtn').forEach(b=>b.innerHTML=IC.x);
paintSound();paintPicker();applyCards();applyTheme();renderGuide();paintKey();paintRem();renderHome();
const hc0=(location.hash||'').slice(1).toUpperCase();if(/^[A-Z]{5}$/.test(hc0)){$('joinCode').value=hc0}
const hcup=/^CUP-([A-Z]{6})$/.exec(hc0);if(hcup){setTab('cups');$('cupCode').value=hcup[1];setTimeout(()=>{if($('intro').hidden)openSheet('cupJoinSheet')},600)}
if(!ST.get('ft-intro',false))openIntro();else if(/^[A-Z]{5}$/.test(hc0))openSheet('friendSheet');
NET.connect();
})();
