import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const root = "static/monaco/vs";

async function jsFiles(dir) {
  const out = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...await jsFiles(path));
    if (entry.isFile() && entry.name.endsWith(".js")) out.push(path);
  }
  return out;
}

function patchClassFieldHelper(source) {
  return source
    .replace(
      /var h=Object\.defineProperty;var m=\(s,e,t\)=>e in s\?h\(s,e,\{enumerable:!0,configurable:!0,writable:!0,value:t\}\):s\[e\]=t;var r=\(s,e,t\)=>\(m\(s,typeof e!="symbol"\?e\+"":e,t\),t\);/,
      'var r=(s,e,t)=>(s[typeof e!="symbol"?e+"":e]=t,t);',
    )
    .replace(
      /var w=Object\.defineProperty;var M=\(o,e,n\)=>e in o\?w\(o,e,\{enumerable:!0,configurable:!0,writable:!0,value:n\}\):o\[e\]=n;var u=\(o,e,n\)=>\(M\(o,typeof e!="symbol"\?e\+"":e,n\),n\);/,
      'var u=(o,e,n)=>(o[typeof e!="symbol"?e+"":e]=n,n);',
    )
    .replace(
      /var h=Object\.defineProperty;var f=\(n,e,t\)=>e in n\?h\(n,e,\{enumerable:!0,configurable:!0,writable:!0,value:t\}\):n\[e\]=t;var i=\(n,e,t\)=>\(f\(n,typeof e!="symbol"\?e\+"":e,t\),t\);/,
      'var i=(n,e,t)=>(n[typeof e!="symbol"?e+"":e]=t,t);',
    )
    .replace(
      /var y=Object\.defineProperty;var C=\(h,i,r\)=>i in h\?y\(h,i,\{enumerable:!0,configurable:!0,writable:!0,value:r\}\):h\[i\]=r;var s=\(h,i,r\)=>\(C\(h,typeof i!="symbol"\?i\+"":i,r\),r\);/,
      'var s=(h,i,r)=>(h[typeof i!="symbol"?i+"":i]=r,r);',
    );
}

function patchDescriptorCopy(source) {
  return source.replace(
    /Object\.defineProperty\(e,s,i\.get\?i:\{enumerable:!0,get:\(\)=>o\[s\]\}\)/g,
    'Object.defineProperty(e,s,i&&i.get?i:{enumerable:!0,get:()=>o[s]})',
  );
}

let patched = 0;
for (const file of await jsFiles(root)) {
  const before = await readFile(file, "utf8");
  const after = patchDescriptorCopy(patchClassFieldHelper(before));
  if (after !== before) {
    await writeFile(file, after);
    patched += 1;
  }
}

console.log(`patched ${patched} Monaco asset(s)`);
