// Collect license text from exactly the Cargo packages selected by metadata.
// CI helper only; the application itself has no Node dependency.
import fs from 'node:fs';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
const target = process.argv[2];
const args=['metadata','--format-version','1','--locked'];
if(target) args.push('--filter-platform',target);
const metadata=JSON.parse(execFileSync('cargo',args,{encoding:'utf8',maxBuffer:64*1024*1024}));
const out=path.resolve('third-party'); fs.mkdirSync(out,{recursive:true});
const inventory=[];
for(const p of metadata.packages){
  if(!p.source) continue;
  const dir=path.dirname(p.manifest_path), dest=path.join(out,`${p.name}-${p.version}`);
  fs.mkdirSync(dest,{recursive:true});
  const files=new Set(fs.readdirSync(dir).filter(n=>/^(license|copying|notice)([._-]|$)/i.test(n)));
  if(p.license_file) files.add(p.license_file);
  const copied=[];
  for(const name of files){
    const source=path.resolve(dir,name);
    if(!source.startsWith(dir+path.sep)||!fs.existsSync(source)||!fs.statSync(source).isFile()) continue;
    const file=path.basename(name); fs.copyFileSync(source,path.join(dest,file)); copied.push(file);
  }
  const item={name:p.name,version:p.version,license:p.license,repository:p.repository,license_files:copied};
  fs.writeFileSync(path.join(dest,'PACKAGE.json'),JSON.stringify(item,null,2)+'\n'); inventory.push(item);
}
fs.writeFileSync(path.join(out,'PACKAGES.json'),JSON.stringify(inventory,null,2)+'\n');
fs.writeFileSync(path.join(out,'README.txt'),'License texts collected from resolved Cargo packages. PACKAGE.json identifies package versions and license declarations. No local filesystem paths are included.\n');
console.log(`Collected license inventory for ${inventory.length} packages.`);
