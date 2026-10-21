'use strict';
/* Five Trails browser client.
   Player data (profile, trophies, XP, records, settings) is stored only in this browser's IndexedDB.
   Online play talks to the stateless match server over a WebSocket; the server runs every online match. */
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
const SV=(p,f='none',w=2.6)=>`<svg viewBox="0 0 24 24" fill="${f}" stroke="currentColor" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${p}</svg>`;
const IC={
 swords:SV('<path d="M4 4l9.5 9.5M4 4h4.5M4 4v4.5M20 4l-9.5 9.5M20 4h-4.5M20 4v4.5M6.5 15.5l2 2M17.5 15.5l-2 2M5 19l2.5-2.5M19 19l-2.5-2.5"/>'),
 cup:SV('<path d="M7 4h10v4a5 5 0 0 1-10 0Z M7 6H4v1a3 3 0 0 0 3 3 M17 6h3v1a3 3 0 0 1-3 3 M12 13v4 M8 20h8"/>'),
 book:SV('<path d="M4 5.5A2.5 2.5 0 0 1 6.5 3H20v15H6.5A2.5 2.5 0 0 0 4 20.5Z M4 20.5V5.5 M8.5 7.5h7"/>'),
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
 return `<div class="card ${v?'':'w'} ${extra}" data-id="${id}" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct};${style}" aria-label="${C.n} ${v||'wager'}"><span class="cv">${v||'×'}</span><span class="ci">${icon(c)}</span>${v?`<span class="art">${icon(c)}</span>`:'<span class="ring"></span>'}</div>`}
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
function ensurePw(S){if(!S.pw)S.pw={sab:[1,1],rush:[1,1]}}
function okSab(S,s){ensurePw(S);return !S.over&&S.turn===s&&S.phase==='play'&&S.pw.sab[s]>0&&S.deck.length>=3&&S.hands[1-s].length>0}
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
function doDraw(S,s,p){clockSpend(S,Date.now());const id=p<0?S.deck.pop():S.disc[p].pop();S.hands[s].push(id);S.jd=-1;S.phase='play';
 if(!S.deck.length)S.over=true;else S.turn=1-s;S.seq++;S.last={seq:S.seq,seat:s,k:'draw',id,p};
 if(S.rushSeat===s||S.over){S.rushUntil=null;S.rushSeat=null;S.rushRem=null}
 if(!S.over&&S.rushNext===S.turn){S.rushUntil=Date.now()+RUSH_MS;S.rushSeat=S.turn;S.rushNext=null}}
function viewFor(S,s){const L=S.last;ensurePw(S);const now=Date.now();return clone({
 pw:{sab:[S.pw.sab[s],S.pw.sab[1-s]],rush:[S.pw.rush[s],S.pw.rush[1-s]]},
 ru:S.rushUntil&&!S.over?{seat:S.rushSeat===s?0:1,l:S.rushRem!=null?S.rushRem:S.rushUntil-now,pz:S.rushRem!=null}:null,
 rn:S.rushNext==null?null:(S.rushNext===s?0:1),gid:S.gid,seq:S.seq,me:S.hands[s],on:S.hands[1-s].length,ex:[S.exp[s],S.exp[1-s]],di:S.disc,dn:S.deck.length,
 mt:S.turn===s,ph:S.phase,jd:S.jd,ov:S.over,why:S.why,fl:S.flag==null?null:(S.flag===s?'me':'opp'),
 ck:S.clk?{b:S.clk.base,l:[clockLeft(S,s,now),clockLeft(S,1-s,now)],run:S.over||S.clk.paused?null:(S.turn===s?0:1),d:Math.max(0,S.clk.since-now)}:null,
 la:L&&{seq:L.seq,by:L.seat===s?'me':'opp',k:L.k,id:(L.k==='draw'&&L.p<0&&L.seat!==s)?-1:L.id,p:L.p,r:(L.k==='sab'&&L.seat!==s)?L.r:-1,auto:!!L.auto}})}
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
 last:()=>{for(let i=0;i<14;i++)noise(.06,180+i*12,'lowpass',.22+i*.012,1,i*.045);tone(98,.6,'sine',.35,.66,60);noise(.6,6000,'highpass',.08,1,.66)}
};
const sfx=(k,m)=>{if(sfxOn&&SFX[k])try{SFX[k](m||1)}catch(e){}};
function eventSfx(ev){const opp=ev.by==='opp',m=opp?.8:1,q=[];
 if(ev.k==='flag'){sfx('flag');return}
 if(ev.k==='sab'){sfx('sab',m);const v=cV(ev.id);if(v===0||v>=9)q.push('dump')}
 if(ev.k==='rush'){sfx('rush',m);if(opp)q.push('alarm')}
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
function toast(t){document.querySelectorAll('.toast').forEach(e=>e.remove());const d=document.createElement('div');d.className='toast';d.textContent=t;document.body.appendChild(d);setTimeout(()=>d.remove(),2800)}
function tween(ms,fn,done){if(reduced){fn(1);done&&done();return}const t0=performance.now();const step=t=>{const k=Math.min(1,(t-t0)/ms);fn(1-Math.pow(1-k,3));if(k<1)requestAnimationFrame(step);else done&&done()};requestAnimationFrame(step)}

/* ================= Player data (IndexedDB, this browser only) ================= */
const REC0=()=>({ai:{w:0,l:0,d:0},pvp:{w:0,l:0,d:0},opp:{},recent:[],tr:0,best:0,xp:0});
function normRec(r){const o=Object.assign(REC0(),r&&typeof r==='object'?r:{});if(r&&r.tr==null){o.tr=Math.max(0,o.ai.w*12-o.ai.l*8+o.pvp.w*30-o.pvp.l*20);o.best=o.tr;o.xp=(o.ai.w+o.ai.l+o.ai.d+o.pvp.w+o.pvp.l+o.pvp.d)*45}return o}
let REC=normRec(ST.get('ft-record',null));
let NAME=ST.get('ft-nick','');
let PID=ST.get('pid',null);
if(typeof PID!=='string'||!/^[A-Za-z0-9-]{8,64}$/.test(PID)){PID=crypto.randomUUID?crypto.randomUUID():Array.from(crypto.getRandomValues(new Uint8Array(16)),b=>b.toString(16).padStart(2,'0')).join('');ST.set('pid',PID)}
const nick=()=>(NAME||'Explorer').slice(0,16);
function saveRec(){ST.set('ft-record',REC)}
const RESULT={};
function recordGame(mode,oppName,oppKey,me,them,gid,r){
 if(RESULT[gid])return RESULT[gid];
 const t0=REC.tr,x0=REC.xp,a0=arenaIdx(t0);
 if(REC.recent.some(x=>x.g===gid))return RESULT[gid]={r,t0,t1:t0,x0,x1:x0,a0,a1:a0,dT:0,dX:0};
 const b=mode==='ai'?REC.ai:REC.pvp;b[r]++;
 if(mode==='pvp'){const o=REC.opp[oppKey]||{name:oppName,w:0,l:0,d:0};o.name=oppName;o[r]++;REC.opp[oppKey]=o}
 const D=mode==='ai'?{w:12,l:-8,d:0}:{w:30,l:-20,d:0};
 const t1=Math.max(ARENAS[a0].at,t0+D[r]);
 const dX=20+(r==='w'?30:r==='d'?15:5)+Math.floor(Math.max(0,Math.min(me,120))/4);
 REC.tr=t1;REC.best=Math.max(REC.best,t1);REC.xp=x0+dX;
 REC.recent.unshift({m:mode,o:oppName,me,them,r,t:Date.now(),g:gid,dt:t1-t0});REC.recent=REC.recent.slice(0,12);saveRec();
 return RESULT[gid]={r,t0,t1,x0,x1:x0+dX,a0,a1:arenaIdx(t1),dT:t1-t0,dX}}
const tot=()=>({w:REC.ai.w+REC.pvp.w,l:REC.ai.l+REC.pvp.l,d:REC.ai.d+REC.pvp.d});
const wld=o=>`${o.w}W · ${o.l}L${o.d?` · ${o.d}D`:''}`;

/* ================= Network: stateless match server ================= */
const SERVER_URL=(()=>{const m=document.querySelector('meta[name="five-trails-server"]');const u=m&&m.content.trim();if(u)return u;
 if(location.protocol!=='http:'&&location.protocol!=='https:')return null;return(location.protocol==='https:'?'wss://':'ws://')+location.host+'/ws'})();
const NET={ws:null,ready:false,stats:null,backoff:800,pingT:null,
 connect(){if(!SERVER_URL||(this.ws&&this.ws.readyState<2))return;let ws;try{ws=new WebSocket(SERVER_URL)}catch(e){netChanged();return}
  this.ws=ws;
  ws.onopen=()=>{this.backoff=800;const h={t:'hello',v:1,name:safeName(),pid:PID,tr:REC.tr,lv:lvlInfo(REC.xp).lv};const tok=SS.get('ft-resume');if(tok)h.resume=tok;this.raw(h);
   clearInterval(this.pingT);this.pingT=setInterval(()=>this.raw({t:'ping'}),25000)};
  ws.onmessage=e=>{if(typeof e.data!=='string'||e.data.length>65536)return;let m;try{m=JSON.parse(e.data)}catch(_){return}if(m&&typeof m.t==='string')onServer(m)};
  ws.onclose=()=>{clearInterval(this.pingT);const was=this.ready;this.ws=null;this.ready=false;netDown(was);netChanged();
   setTimeout(()=>this.connect(),this.backoff+Math.random()*400);this.backoff=Math.min(15000,this.backoff*2)};
  ws.onerror=()=>{}},
 raw(m){if(this.ws&&this.ws.readyState===1){try{this.ws.send(JSON.stringify(m))}catch(e){}}},
 send(m){if(!this.ready){toast(SERVER_URL?'Still connecting to the game server. Try again in a moment.':'Online play needs the game server.');return false}this.raw(m);return true}};
function sendProfile(){NET.raw({t:'profile',name:safeName(),tr:REC.tr,lv:lvlInfo(REC.xp).lv})}
function safeName(){const n=nick().replace(/[^\p{L}\p{N} \-_.']/gu,'').replace(/\s+/g,' ').trim().slice(0,16);return n||'Explorer'}
const ERRS={room_not_found:'No open room has that code. Check it and try again.',too_many_attempts:'Too many wrong codes. Wait a minute and try again.',busy:'Finish what you are doing first.',
 server_full:'The server is full right now. Try again soon.',bad_hello:'The server did not accept your name. Try a simpler one in Settings.',version:'This page is out of date. Reload to play online.',
 opponent_left:'Your opponent has left.',opponent_away:'Waiting for your opponent to reconnect.',rate_limited:'Slow down a little.',replaced:'This battle continued in another tab.'};
function onServer(m){switch(m.t){
 case 'welcome':NET.ready=true;NET.stats=m.stats||null;netChanged();break;
 case 'stats':NET.stats=m.stats||null;netChanged();break;
 case 'queued':M={mode:'queue'};showWait('Finding an opponent','','Pairing you with a player near your trophy count who picked the same clock…');break;
 case 'unqueued':case 'room_closed':if(M.mode==='queue'||M.mode==='room')goHome(true);break;
 case 'room':M={mode:'room',code:String(m.code||'').replace(/[^A-Z]/g,'')};showWait('Room code',M.code,'Share this code with your friend. Waiting for them to join…');break;
 case 'room_expired':if(M.mode==='room'){goHome(true);toast('Your room expired. Create a new one any time.')}break;
 case 'match':onMatch(m);break;
 case 'state':if(M.mode==='online'&&m.view&&typeof m.view==='object'){V=m.view;V.at=Date.now();pending=false;M.reconnecting=false;renderGame();updateRmNote()}break;
 case 'emote':if(M.mode==='online'&&Number.isInteger(m.k)&&EMOTES[m.k])showBubble('opp',EMOTES[m.k]);break;
 case 'opp':if(M.mode==='online'){M.oppHere=!!m.connected;M.oppLeft=!!m.left;if(V)renderGame();updateRmNote()}break;
 case 'rematch':if(M.mode==='online'&&V){M.iWantRm=m.me?V.gid:null;M.theyWantRm=m.opp?V.gid:null;updateRmNote()}break;
 case 'closed':if(M.mode==='online'){M.closed=true;SS.set('ft-resume',null);updateRmNote()}break;
 case 'resume_failed':SS.set('ft-resume',null);if(M.mode==='online'&&!(V&&V.ov)){goHome(true);toast('That battle has ended.')}break;
 case 'error':pending=false;if(V&&M.mode==='online')renderActs();if(m.code!=='stale'&&m.code!=='rate_limited')toast(ERRS[m.code]||'That move was not allowed.');break;
}}
function onMatch(m){const v=m.view,o=m.opp;if(!v||typeof v!=='object'||!o)return;SS.set('ft-resume',typeof m.token==='string'?m.token:null);
 const opp={nick:String(o.name||'Opponent').slice(0,16),tr:Math.max(0,Number(o.tr)||0),lv:Math.max(1,Number(o.lv)||1),pid:String(o.pid||'').slice(0,64)};
 if(M.mode!=='online')M={mode:'online',oppEmoT:undefined};
 Object.assign(M,{opp,oppKey:'p:'+opp.pid,kind:m.kind==='room'?'room':'quick',code:typeof m.code==='string'?m.code.replace(/[^A-Z]/g,''):null,
  oppHere:m.oppConnected!==false,oppLeft:false,closed:false,reconnecting:false,iWantRm:null,theyWantRm:null});
 V=v;V.at=Date.now();pending=false;sel=null;
 $('roomTag').textContent=M.kind==='room'?'Room '+M.code:'Quick match';
 if($('game').hidden)show('game');
 renderGame()}
function netDown(wasReady){if(M.mode==='queue'||M.mode==='room'){goHome(true);toast('Lost connection to the game server.')}
 else if(M.mode==='online'){if(V&&V.ov){M.closed=true;updateRmNote()}else{M.reconnecting=true;if(V)renderGame()}}}
function netChanged(){const ok=NET.ready,s=NET.stats;
 ['quickBtn','create','join','joinCode'].forEach(id=>{const e=$(id);if(e)e.disabled=!ok});
 $('onlineLine').innerHTML=!SERVER_URL?'<span class="netdot"></span>Needs the game server':ok?`<span class="netdot on"></span>${s?Number(s.online)||0:1} online now`:'<span class="netdot wait"></span>Connecting…';
 $('roomNote').textContent=!SERVER_URL?'Open this page from the game server address to play online.':ok?(s?`${Number(s.online)||0} online · ${Number(s.searching)||0} searching · ${Number(s.playing)||0} in battle. Quick match uses your clock setting.`:'Connected.'):'Connecting to the game server…'}
function showWait(label,code,msg){show('wait');$('waitLbl').textContent=label;$('waitCode').textContent=code||'';$('waitCode').hidden=!code;$('waitMsg').textContent=msg}

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
 renderRecords();
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
 <div class="pn"><div class="pn-h ol">Arena road</div><div class="road">${ARENAS.map((A,i)=>`<div class="rd ${REC.tr>=A.at?'open':'lock'} ${i===ai?'cur':''}">${emblem(i,40,'embs')}<div><b>${A.n}</b><span>${TROPHY}${A.at} trophies · ${AI_TIER[i]} computer</span></div>${i===ai?'<em>You are here</em>':REC.tr>=A.at?'':'<em class="lk">Locked</em>'}</div>`).join('')}</div></div>`}
function renderGuide(){
 const ex=(cards,math)=>`<div class="ex"><div class="cards">${cards.map(id=>mini(id,34)).join('')}</div><div class="math">${math}</div></div>`;
 $('tab-guide').innerHTML=`
 <div class="pn gsec"><div class="pn-h ol">How a turn works</div>
  <p><b>1. Play one card.</b> Add it to your trail of that color, or discard it onto that color's pile.</p>
  <p><b>2. Draw one card</b> from the deck or the top of any discard pile, except the pile you just discarded to.</p>
  <p>Every card in a trail must be higher than the one before it. Wager cards can only go first. The battle ends the moment the last deck card is drawn.</p></div>
 <div class="pn gsec"><div class="pn-h ol">Scoring</div>
  <p>Each trail you start costs 20. A trail scores (sum − 20) × (1 + wagers). Eight or more cards add 20. Trails you never start score 0.</p>
  ${ex([cid(0,3),cid(0,6),cid(0,8),cid(0,10)],'Desert: 27 − 20 = <b>7</b>')}
  ${ex([cid(1,0,0),cid(1,4),cid(1,7),cid(1,9)],'Sea with 1 wager: (20 − 20) × 2 = <b>0</b>')}
  ${ex([cid(2,0,0),cid(2,0,1),cid(2,5),cid(2,7),cid(2,9),cid(2,10)],'Jungle with 2 wagers: (31 − 20) × 3 = <b>33</b>')}
  ${ex([cid(3,0,0),cid(3,2),cid(3,4)],'Volcano with 1 wager: (6 − 20) × 2 = <b>−28</b>')}</div>
 <div class="pn gsec"><div class="pn-h ol">Powerups</div>
  <p>You get one of each per battle. Tap the button, then tap again to confirm.</p>
  <p><span class="pwb">SABOTAGE</span> Before you play, knock a random card from your rival's hand onto its discard pile. They draw a replacement.</p>
  <p><span class="pwb">RUSH</span> Your rival's next turn lasts only 10 seconds. If time runs out, a random card of theirs is discarded and they draw from the deck.</p></div>
 <div class="pn gsec"><div class="pn-h ol">Trophies and arenas</div>
  <p>Beat the computer for +12 trophies, lose for −8. Against players it's +30 and −20. Once you reach an arena you never drop below it.</p>
  <p>Each new arena brings a sharper computer opponent. Every battle earns XP, and more points earn more XP.</p>
  <p>With the clock on, your time only runs on your turn. If it hits zero, you lose.</p>
  <button class="gb blue sm" id="replayIntro2" style="align-self:flex-start">Replay the intro</button></div>`;
 $('replayIntro2').onclick=()=>{sfx('ui');openIntro()}}
function setTab(t){document.querySelectorAll('.nav button').forEach(b=>b.setAttribute('aria-selected',String(b.dataset.tab===t)));
 for(const k of ['battle','records','guide'])$('tab-'+k).hidden=k!==t;try{scrollTo({top:0})}catch(e){}}
function paintPicker(){document.querySelectorAll('#clkpick button').forEach(b=>b.setAttribute('aria-checked',String(+b.dataset.clk===clkSet)))}

/* Sheets */
function openSheet(id){closeSheets();$(id).hidden=false;sfx('swoosh');const f=$(id).querySelector('input,button.gb:not(.xbtn)');if(f&&id!=='leaveSheet')setTimeout(()=>{try{f.focus({preventScroll:true})}catch(e){}},50)}
function closeSheets(){document.querySelectorAll('.sheet').forEach(s=>{if(s.id!=='intro')s.hidden=true})}

/* Intro */
let introI=0;
const INTRO=[
 {t:'Welcome, explorer!',v:()=>`<div style="position:relative;width:240px;height:130px">${[cid(0,10),cid(1,9),cid(2,8),cid(3,7),cid(4,6)].map((id,i)=>mini(id,58,`position:absolute;left:91px;bottom:0;transform-origin:50% 150%;transform:rotate(${(i-2)*13}deg)`)).join('')}</div>`,p:'Five colored trails, one shared deck. Build expeditions and finish with more points than your rival.'},
 {t:'Always climb',v:()=>[cid(1,3),cid(1,6),cid(1,9)].map((id,i)=>mini(id,56,`transform:translateY(${(1-i)*16}px)`)).join(''),p:'Every card you add to a trail must be higher than the one before it. Plan your order.'},
 {t:'Bet bold',v:()=>[cid(3,0,0),cid(3,7),cid(3,10)].map(id=>mini(id,56)).join(''),p:'Each trail you start costs 20 points. Wager cards go first and multiply a trail, win or lose.'},
 {t:'Two tricks per battle',v:()=>`<div style="display:flex;flex-direction:column;gap:10px;align-items:center"><span class="pwb" style="font-size:18px;padding:5px 14px">SABOTAGE</span><span class="pwb" style="font-size:18px;padding:5px 14px">RUSH</span></div>`,p:"Sabotage knocks a random rival card onto the discard pile. Rush gives your rival just 10 seconds on their next turn."},
 {t:"What's your name?",name:true}
];
function openIntro(){introI=0;closeSheets();$('intro').hidden=false;renderIntro()}
function renderIntro(){const s=INTRO[introI];const dots=`<div class="dots">${INTRO.map((_,i)=>`<i class="${i===introI?'on':''}"></i>`).join('')}</div>`;
 $('introPn').innerHTML=s.name?`<div class="islide"><h2>${s.t}</h2><div class="ivis">${avatarHTML(NAME||'E',null).replace('class="av"','class="av" style="width:84px;height:84px;font-size:44px;border-radius:22px"')}</div>
   <input id="introName" maxlength="16" placeholder="Explorer" value="${esc(NAME)}" aria-label="Your name" style="width:100%;text-align:center">${dots}
   <div class="introbtns"><button class="gb go" id="introGo">Let's go!</button></div></div>`
 :`<div class="islide"><h2>${s.t}</h2><div class="ivis">${s.v()}</div><p>${s.p}</p>${dots}<div class="introbtns"><button class="gb gray" id="introSkip">Skip</button><button class="gb green" id="introNext">Next</button></div></div>`;
 if(s.name){const inp=$('introName');inp.oninput=()=>{const av=$('introPn').querySelector('.av');if(av)av.firstChild.textContent=((inp.value||'E').trim()[0]||'E').toUpperCase()};
  inp.onkeydown=e=>{if(e.key==='Enter')$('introGo').click()};
  $('introGo').onclick=()=>{NAME=inp.value.trim().slice(0,16);ST.set('ft-nick',NAME);ST.set('ft-intro',true);$('intro').hidden=true;$('nick').value=NAME;sfx('go');renderHome();burst(innerWidth/2,innerHeight/2,['#ffd23a','#7dffa0','#4f9dff','#ff5d6c'],30,{sp:8})}}
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
function oppInfo(){const ai=arenaIdx(REC.tr);
 if(M.mode==='ai')return{name:'Computer',lv:null,tr:null,cpu:true,sub:`${AI_TIER[M.tier??ai]} bot`};
 const o=M.opp||{};const h=M.oppKey&&REC.opp[M.oppKey];return{name:o.nick||'Opponent',lv:o.lv||null,tr:o.tr??null,cpu:false,sub:`${o.tr!=null?TROPHY+(Number(o.tr)||0):''}${h?` · You ${h.w}–${h.l}`:''}`}}
const meInfo=()=>({name:nick(),lv:lvlInfo(REC.xp).lv});
function setScore(id,v){const el=$(id);if(el.textContent!==String(v)){el.textContent=v;el.classList.remove('bump');void el.offsetWidth;el.classList.add('bump')}}
function renderGame(){
 if(!V)return;
 const prev=snap();
 const newGame=V.gid!==prevGid;const ev=!newGame&&V.la&&V.la.seq>prevSeq?V.la:null;
 const myT=V.mt&&!V.ov;
 const opp=oppInfo(),me_=meInfo();
 if(newGame)M.oppLast=null;
 if(ev&&ev.by==='opp'){if(!M.oppLast||M.lastBy!=='opp')M.oppLast={};const L=M.oppLast;
  if(ev.k==='play'||ev.k==='disc')L.act=ev;else if(ev.k==='draw')L.draw=ev;else if(ev.k==='sab')L.sab=ev;else if(ev.k==='rush')L.rush=true}
 if(ev)M.lastBy=ev.by;
 $('oppAv').innerHTML=avatarHTML(opp.name,opp.lv,opp.cpu);$('oppName').textContent=opp.name;$('oppSub').innerHTML=opp.sub;
 $('meAv').innerHTML=avatarHTML(me_.name,me_.lv);$('meName').textContent=me_.name;$('meSub').innerHTML=`${TROPHY}${REC.tr}`;
 $('oppHand').innerHTML='<div class="back"></div>'.repeat(V.on);
 setScore('oppScore',total(V.ex[1]));setScore('meScore',total(V.ex[0]));
 const pwb=i=>{const p=V.pw||{sab:[0,0],rush:[0,0]};return `<span class="pwb ${p.sab[i]?'':'used'}">SABOTAGE</span><span class="pwb ${p.rush[i]?'':'used'}">RUSH</span>${V.rn===i?'<span class="pwb hit">RUSHED</span>':''}`};
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
 if(M.oppLast&&M.oppLast.act){const el=document.querySelector(`#board [data-id="${M.oppLast.act.id}"]`);if(el)el.classList.add('last')}
 const hand=V.me.slice().sort((a,b)=>cC(a)-cC(b)||cV(a)-cV(b));
 if(sel!=null&&!hand.includes(sel))sel=null;
 $('hand').innerHTML=hand.map(id=>cardHTML(id,sel===id?'sel':'').replace('<div ','<div role="button" tabindex="0" data-h="1" ')).join('');
 $('hand').classList.toggle('off',!(myT&&V.ph==='play'));
 renderActs();
 let m='';
 if(V.ov)m=V.fl==='me'?(V.why==='forfeit'?'<b>You left the battle.</b>':'<b>Your time ran out.</b>'):V.fl==='opp'?(V.why==='forfeit'?`<b>${esc(opp.name)} left the battle.</b>`:`<b>${esc(opp.name)} ran out of time.</b>`):'<b>Battle over!</b>';
 else if(M.mode==='online'&&M.reconnecting)m='Connection lost. Reconnecting…';
 else if(M.mode!=='ai'&&!M.oppHere)m=`${esc(opp.name)} disconnected. Waiting for them to return…`;
 else if(myT)m=V.ph==='play'?(lastText(opp.name)?`<b>Your turn.</b> ${lastText(opp.name)}`:'<b>Your turn.</b> Play or discard a card.'):'<b>Draw</b> from the deck or a discard pile.';
 else m=M.mode==='ai'?'Computer is thinking…':`${esc(opp.name)}'s turn…`;
 $('msg').className='msg';$('msg').innerHTML=m;

 const deckR=rectOf($('deck')),oppR=rectOf($('oppHand'));
 const now=new Set();
 if(newGame){PS=null;closeEmo();$('over').hidden=true;
  if(M.skipVS){M.skipVS=false;dealAnim()}else vsSplash(dealAnim)}
 else document.querySelectorAll('#game [data-id]').forEach(el=>{const id=el.dataset.id;now.add(id);const p=prev.get(id);
  if(p)animateFrom(el,p.r);else if(ev)animateFrom(el,el.closest('#hand')?deckR:oppR)});
 if(!newGame){prev.forEach((p,id)=>{if(!now.has(id))ghost(p.h,p.r,oppR,true)});
  if(ev&&ev.k==='sab'&&ev.by==='me')ghost('<div class="back" style="border-radius:8px"></div>',deckR,oppR,true);
  if(ev&&ev.by==='opp'&&ev.k==='draw'&&ev.p<0)ghost('<div class="back" style="border-radius:8px"></div>',deckR,oppR,true)}
 if(ev){eventSfx(ev);fxEvent(ev,opp);if(ev.by==='opp'&&ev.k==='draw'&&!V.ov)setTimeout(()=>sfx('turn'),250)}
 scorePops();
 prevGid=V.gid;prevSeq=V.seq;
 if(V.ov)setTimeout(showResult,ev?1800:200);
}
function lastText(name){const L=M.oppLast;if(!L)return '';const ch=id=>{const C=COL[cC(id)];return `<span class="mchip" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct}">${C.n} ${cV(id)||'wager'}</span>`};const parts=[];
 if(L.sab)parts.push(`knocked out your ${ch(L.sab.id)}`);if(L.rush)parts.push('used Rush');
 if(L.act)parts.push(`${L.act.k==='play'?'played':'discarded'} ${ch(L.act.id)}`);
 if(L.draw)parts.push(L.draw.auto?'ran out of rush time':L.draw.p>=0?`took ${ch(L.draw.id)} from the discards`:'drew from the deck');
 if(!parts.length)return '';const t=parts.length>1?parts.slice(0,-1).join(', ')+' and '+parts[parts.length-1]:parts[0];return `${esc(name)} ${t}.`}
function scorePops(){const cur=[0,1].map(s=>V.ex[s].map(expScore)),lens=[0,1].map(s=>V.ex[s].map(e=>e.length));
 if(PS&&PS.g===V.gid)for(let s=0;s<2;s++)for(let c=0;c<5;c++){if(lens[s][c]>PS.l[s][c]){const d=cur[s][c]-PS.v[s][c];if(!d)continue;
   const el=document.querySelectorAll(`#board .col[data-c="${c}"] .cs`)[s?0:1];setTimeout(()=>popup(el,(d>0?'+':'−')+Math.abs(d),d>0?'up':'dn'),reduced?0:430)}}
 PS={g:V.gid,v:cur,l:lens}}
function fxEvent(ev,opp){const side=ev.by==='me'?0:1,D=reduced?0:430,who=side?opp.name:'';const nm=id=>COL[cC(id)].n+' '+(cV(id)||'wager');
 if(ev.k==='play'){const c=ev.p,C=COL[c],v=cV(ev.id),e=V.ex[side][c];
  setTimeout(()=>{const el=document.querySelector(`#board .col[data-c="${c}"] .stack.${side?'opp':'me'} [data-id="${ev.id}"]`);
   if(el){const[x,y]=centerOf(el);burst(x,y,[C.cc,C.cd,'#ffffff'],v===0||v>=9?28:14,{sp:v>=9?7.5:5})}if(v>=9)shake(2)},D);
  if(v===0)callout(`Wager ×${e.filter(id=>cV(id)===0).length+1}`,C.cc,who);
  if(e.length===8)callout('Bonus +20','#7dffa0',who);
  if(side===0&&(v>=9||e.length===8))aiSay(1,.3)}
 else if(ev.k==='disc'){setTimeout(()=>{const el=document.querySelector(`#board .pile[data-pile="${ev.p}"]`);if(el){const[x,y]=centerOf(el);burst(x,y,['#c9d6f5','#8ea3d6'],8,{sp:3,up:1})}},D)}
 else if(ev.k==='sab'){callout('Sabotage!','#d29bff',side?`Your ${nm(ev.id)} is out`:`Their ${nm(ev.id)} is out`);
  setTimeout(()=>{const el=document.querySelector(`#board .pile[data-pile="${ev.p}"]`);if(el){const[x,y]=centerOf(el);burst(x,y,['#bb73ff','#8a3dff','#fff'],24,{sp:7})}shake(2)},D);
  if(side)aiSay(3,.45)}
 else if(ev.k==='rush'){callout(side?'Rushed!':'Rush!','#ff7aa8',side?'Your next turn: 10 seconds':'Their next turn: 10 seconds');shake(1)}
 else if(ev.k==='flag'){callout(V.why==='forfeit'?'Forfeit':'Time!','#ff5d6c')}
 else if(ev.k==='draw'){if(ev.auto)callout('Too slow!','#ff7aa8',side?`${opp.name} lost a card`:'A random card was discarded');
  if(ev.p>=0)setTimeout(()=>{const el=side?$('oppHand'):document.querySelector(`#hand [data-id="${ev.id}"]`);if(el){const[x,y]=centerOf(el);burst(x,y,[COL[ev.p].cc,'#fff'],10,{sp:4})}},D);
  if(V.ov)callout('Final card!','#ffd23a')}}
function dealAnim(){const deckR=rectOf($('deck'));let i=0;document.querySelectorAll('#hand .card').forEach(el=>animateFrom(el,deckR,i++*70));
 document.querySelectorAll('#oppHand .back').forEach((el,j)=>animateFrom(el,deckR,j*70));sfx('deal');
 setTimeout(()=>{if(!V||V.ov)return;callout(V.mt?'Your turn!':(M.mode==='ai'?'Computer starts':`${oppInfo().name} starts`),V.mt?'#7dffa0':'#ffd23a');if(V.mt)sfx('go');aiSay(0,.5)},650)}
function vsSplash(cb){const o=$('vs'),opp=oppInfo(),me_=meInfo(),ai=arenaIdx(REC.tr);
 const tag=M.mode==='ai'?`${ARENAS[M.tier??ai].n} · ${AI_TIER[M.tier??ai]} computer`:M.kind==='room'?`Room ${M.code||''} · Friendly battle`:'Quick match · Ranked';
 o.innerHTML=`<div class="vs-in">
  <div class="vs-band top"><div class="vs-pl">${avatarHTML(opp.name,opp.lv,opp.cpu)}<div><div class="vs-n ol">${esc(opp.name)}</div><div class="vs-t ol">${opp.cpu?esc(opp.sub):TROPHY+(Number(opp.tr)||0)}</div></div></div></div>
  <div class="vs-mid ol">VS</div>
  <div class="vs-band bot"><div class="vs-pl">${avatarHTML(me_.name,me_.lv)}<div><div class="vs-n ol">${esc(me_.name)}</div><div class="vs-t ol">${TROPHY}${REC.tr}</div></div></div></div>
  <div class="vs-ar ol">${esc(tag)}</div></div>`;
 o.hidden=false;o.classList.remove('out');sfx('swoosh');setTimeout(()=>sfx('vs'),380);
 setTimeout(()=>{if(!reduced)burst(innerWidth/2,innerHeight/2,['#ffd23a','#fff','#ffaa00'],30,{sp:9})},520);
 setTimeout(()=>{o.classList.add('out');setTimeout(()=>{o.hidden=true;o.classList.remove('out');cb&&cb()},300)},reduced?600:2100)}
function renderActs(){const myT=V&&V.mt&&!V.ov;let h='';
 if(myT&&V.ph==='play'&&sel!=null&&!pending)h=`<button class="gb green" id="aPlay">Play to ${COL[cC(sel)].n}</button><button class="gb gray" id="aDisc">Discard</button>`;
 else if(V&&V.ov)h=`<button class="gb" id="aAgain">${M.mode==='ai'?'Battle again':'Rematch'}</button>`;
 else if(myT&&!pending){const p=V.pw||{sab:[0,0],rush:[0,0]};
  if(V.ph==='play'&&p.sab[0]>0&&V.dn>=3)h+=`<button class="gb purple ${armed==='sab'?'armed':''}" id="aSab">${armed==='sab'?'Tap to confirm':'Sabotage'}</button>`;
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
 return recordGame(M.mode==='ai'?'ai':'pvp',o.name,M.mode==='ai'?'ai':(M.oppKey||'n:'+o.name),me,them,V.gid,r)}
function showResult(){if(!V||!V.ov||M.mode==='home')return;if(shownFor===V.gid&&!$('over').hidden)return;
 const first=shownFor!==V.gid;shownFor=V.gid;const info=finalize();const{me,them,r}=outcome(),o=oppInfo();
 if(first){sfx(r==='l'?'lose':'win');if(r==='w')confetti();if(M.mode==='ai')setTimeout(()=>aiSay(5,.75,true),900)}
 const title=r==='w'?'Victory!':r==='l'?'Defeat':'Draw';
 const reason=V.fl?(V.why==='forfeit'?(V.fl==='me'?'You left the battle':`${o.name} left the battle`):(V.fl==='me'?'Your clock ran out':`${o.name}'s clock ran out`)):r==='d'?'Dead even':`${Math.abs(me-them)} point ${r==='w'?'win':'loss'}`;
 const lanes=COL.map((C,c)=>`<div class="rc" style="--cc:${C.cc};--cd:${C.cd};--ct:${C.ct}"><b>${V.ex[0][c].length?expScore(V.ex[0][c]):'–'}</b><span>${V.ex[1][c].length?expScore(V.ex[1][c]):'–'}</span></div>`).join('');
 const h=M.mode!=='ai'&&M.oppKey&&REC.opp[M.oppKey];
 const L0=lvlInfo(info?info.x0:REC.xp);
 $('over').innerHTML=`<div class="res ${r}">
  <div class="rib ol">${title}</div><div class="rsub">${esc(reason)}</div>
  <div class="rscore"><div class="rs-p ${r==='w'?'win':''}">${avatarHTML(nick(),lvlInfo(REC.xp).lv)}<b class="ol">${me}</b><span>You</span></div><div class="rs-vs">vs</div><div class="rs-p ${r==='l'?'win':''}">${avatarHTML(o.name,o.lv,o.cpu)}<b class="ol">${them}</b><span>${esc(o.name)}</span></div></div>
  <div class="rcols" aria-label="Trail scores, yours on top">${lanes}</div>
  <div class="rrow">${TROPHY}<div class="bar"><i id="rTrBar"></i><b class="ols" id="rTrTxt"></b></div><span class="dchip ${info&&info.dT<0?'neg':info&&info.dT===0?'zero':''}" id="rTrD">${info?(info.dT>0?'+':'')+info.dT:'0'}</span></div>
  <div class="rrow"><div class="lvbox ol" id="rLv">${L0.lv}</div><div class="bar xp"><i id="rXpBar"></i><b class="ols" id="rXpTxt"></b></div><span class="dchip xp">+${info?info.dX:0} XP</span></div>
  ${h?`<p class="note" style="text-align:center">Head to head with ${esc(o.name)}: ${wld(h)}</p>`:''}
  <div class="rbtns"><button class="gb green" id="oAgain">${M.mode==='ai'?'Battle again':'Rematch'}</button><button class="gb blue" id="oHome">Lobby</button></div>
  <button class="linkbtn" id="oView">See the table</button><p class="note" id="rmNote" style="text-align:center"></p></div>`;
 $('over').hidden=false;
 $('oAgain').onclick=()=>{sfx('ui');again()};$('oHome').onclick=()=>{sfx('ui');goHome()};$('oView').onclick=()=>{$('over').hidden=true};
 updateRmNote();
 if(info)animateGains(info,first)}
function trState(t){const a=arenaIdx(t),A=ARENAS[a],N=ARENAS[a+1];return{pct:N?(t-A.at)/(N.at-A.at)*100:100,txt:N?`${t} / ${N.at}`:`${t}`}}
function animateGains(info,first){const set=(t,x)=>{const s=trState(Math.round(t));if(!$('rTrBar'))return;$('rTrBar').style.width=s.pct+'%';$('rTrTxt').textContent=s.txt;const L=lvlInfo(Math.round(x));$('rXpBar').style.width=(L.into/L.need*100)+'%';$('rXpTxt').textContent=`${L.into} / ${L.need} XP`;
  if(+$('rLv').textContent!==L.lv){$('rLv').textContent=L.lv;$('rLv').classList.remove('up');void $('rLv').offsetWidth;$('rLv').classList.add('up');if(first){sfx('levelup');callout('Level up!','#7fd0ff',`Level ${L.lv}`)}}};
 if(!first){set(info.t1,info.x1);return}
 set(info.t0,info.x0);let lastT=info.t0;
 setTimeout(()=>tween(900,k=>{const t=info.t0+(info.t1-info.t0)*k;if(Math.round(t)!==Math.round(lastT)&&Math.round(t)%3===0)sfx('coin');lastT=t;set(t,info.x0)},()=>{
  tween(1000,k=>set(info.t1,info.x0+(info.x1-info.x0)*k),()=>{
   if(info.a1>info.a0){sfx('arena');confetti();callout('New arena!',ARENAS[info.a1].g[0],ARENAS[info.a1].n)}})}),700)}
function updateRmNote(){const n=$('rmNote');if(!n||!V)return;n.textContent=M.mode==='ai'?'':(M.closed||M.oppLeft)?'Your opponent has left the battle.':M.iWantRm===V.gid?(M.theyWantRm===V.gid?'Starting…':'Waiting for your opponent…'):(M.theyWantRm===V.gid?'Your opponent wants a rematch!':'')}

/* ================= Actions ================= */
function act(kind,arg,x,el){
 if(!V||V.ov||pending)return;armed=null;
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
 const playStep=()=>{if(!alive())return;const b=aiChoosePlay(S,AI_NOISE[M.tier||0]);doPlay(S,1,b.id,b.x);rr();
  aiT=setTimeout(()=>{if(!alive())return;doDraw(S,1,aiChooseDraw(S));rr()},780)};
 const sabStep=()=>{if(!alive())return;const L=S.deck.length;if(okSab(S,1)&&L<=30&&L>=6&&Math.random()<.25){doSab(S,1);rr();aiT=setTimeout(playStep,1300)}else playStep()};
 const wait=Math.max(850,(S.t0||0)-Date.now()+1300);
 aiT=setTimeout(()=>{if(!alive())return;if(okRush(S,1)&&S.deck.length<=34&&Math.random()<.2){doRush(S,1);rr();aiT=setTimeout(sabStep,1200)}else sabStep()},wait)}
function again(){$('over').hidden=true;
 if(M.mode==='ai'){startAi();return}
 if(!V||!V.ov)return;
 if(M.closed||M.oppLeft||!NET.ready){toast('Your opponent has left. Find a new match from the lobby.');showResult();return}
 if(NET.send({t:'rematch'}))M.iWantRm=V.gid;
 showResult()}
function askLeave(){if(V&&!V.ov&&(M.mode==='ai'||M.mode==='online'))openSheet('leaveSheet');else goHome()}
function forfeit(){closeSheets();if(!V||V.ov){goHome();return}
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
 const ok=V&&!$('game').hidden;const now=Date.now();
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
$('create').onclick=()=>{sfx('ui');if(NET.ready)sendProfile();if(NET.send({t:'create',clock:clkSet}))closeSheets()};
$('quickBtn').onclick=()=>{sfx('ui');if(NET.ready)sendProfile();if(NET.send({t:'queue',clock:clkSet}))closeSheets()};
$('join').onclick=()=>{const c=$('joinCode').value.trim().toUpperCase();if(!/^[A-Z]{6}$/.test(c)){sfx('bad');$('joinCode').classList.remove('shake');void $('joinCode').offsetWidth;$('joinCode').classList.add('shake');$('joinCode').focus();return}sfx('ui');if(NET.ready)sendProfile();if(NET.send({t:'join',code:c}))closeSheets()};
$('joinCode').addEventListener('keydown',e=>{if(e.key==='Enter')$('join').click()});
$('joinCode').addEventListener('input',e=>{e.target.value=e.target.value.toUpperCase().replace(/[^A-Z]/g,'')});
$('nick').value=NAME;$('nick').addEventListener('input',()=>{NAME=$('nick').value.trim().slice(0,16);ST.set('ft-nick',NAME);renderTop()});
$('topProf').onclick=$('gearBtn').onclick=()=>{sfx('ui');$('nick').value=NAME;openSheet('setSheet')};
$('replayIntro').onclick=()=>{sfx('ui');openIntro()};
document.querySelectorAll('.nav button').forEach(b=>b.onclick=()=>{if(b.getAttribute('aria-selected')!=='true')sfx('ui');setTab(b.dataset.tab)});
$('clkpick').addEventListener('click',e=>{const b=e.target.closest('button');if(!b)return;clkSet=+b.dataset.clk;ST.set('ft-clock',clkSet);paintPicker();sfx('select')});
const goHome=(quiet)=>{leaveRoom();if(!quiet){if(M.mode==='online')NET.raw({t:'leave_match'});else if(M.mode==='queue')NET.raw({t:'unqueue'});else if(M.mode==='room')NET.raw({t:'cancel_room'})}
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
$('navBattle').innerHTML=IC.swords+'Battle';$('navRecords').innerHTML=IC.cup+'Records';$('navGuide').innerHTML=IC.book+'Guide';
$('gearBtn').innerHTML=IC.gear;$('leave').innerHTML=IC.x;$('emoBtn').innerHTML=IC.chat;$('clkLbl').innerHTML=IC.clock+'Clock each';
document.querySelectorAll('.xbtn').forEach(b=>b.innerHTML=IC.x);
paintSound();paintPicker();renderGuide();renderHome();
const hc0=(location.hash||'').slice(1).toUpperCase();if(/^[A-Z]{6}$/.test(hc0)){$('joinCode').value=hc0}
if(!ST.get('ft-intro',false))openIntro();else if(/^[A-Z]{6}$/.test(hc0))openSheet('friendSheet');
NET.connect();
})();
