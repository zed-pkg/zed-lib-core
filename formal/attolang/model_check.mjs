// Finite graph document/edge atomic-admission reference, NOT a DB refinement.
import assert from 'node:assert/strict';
const init={document:false,edges:false,digest:false,committed:false,sealed:false,mutated_after_seal:false};
const actions=['validate_document','normalize_edges','check_digest','commit','seal','try_mutate'];
const key=s=>JSON.stringify(s);
function step(s,a){
 if(s.sealed)return s;
 if(a==='validate_document')return {...s,document:true};
 if(a==='normalize_edges')return {...s,edges:true};
 if(a==='check_digest')return {...s,digest:true};
 if(a==='commit'&&s.document&&s.edges&&s.digest)return {...s,committed:true};
 if(a==='seal'&&s.committed)return {...s,sealed:true};
 return s;
}
const safe=s=>(!s.committed||(s.document&&s.edges&&s.digest)) &&
 (!s.sealed||s.committed) && !s.mutated_after_seal;
function verify(next){
 const seen=new Map([[key(init),init]]),q=[init];
 for(let i=0;i<q.length;i++)for(const a of actions){
  const s=q[i],n=next(s,a);
  assert.ok(safe(n),`unsafe ${a}: ${key(n)}`);
  assert.ok(!s.sealed||key(n)===key(s),'post-seal mutation');
  if(!seen.has(key(n))){assert.ok(seen.size<128,'state bound');seen.set(key(n),n);q.push(n);}
 }
 return [...seen.values()];
}
const states=verify(step);
assert.ok(states.length>=8&&states.some(s=>s.sealed));
assert.ok(states.some(s=>s.document&&!s.edges&&!s.committed));
assert.throws(()=>verify((s,a)=>a==='commit'?{...s,committed:true}:step(s,a)),/unsafe/);
console.log(`model=graph_commit_admission states=${states.length} negative_controls=1 status=PASS scope=abstract`);
