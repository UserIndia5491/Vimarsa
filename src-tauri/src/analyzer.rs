use anyhow::{anyhow, Context, Result};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::{BTreeMap, BTreeSet, HashMap}, fs, path::{Path, PathBuf}};

const MAX_SOURCE_BYTES: u64 = 1_500_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoReport {
    pub name: String, pub root: String, pub summary: RepoSummary,
    pub languages: Vec<LanguageStat>, pub files: Vec<FileReport>,
    pub modules: Vec<ModuleReport>, pub edges: Vec<DependencyEdge>,
    pub architecture: Vec<ArchitectureInsight>, pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoSummary {
    pub files: usize, pub directories: usize, pub source_files: usize,
    pub total_bytes: u64, pub largest_file: Option<String>,
    pub detected_frameworks: Vec<String>, pub confidence: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageStat { pub language: String, pub files: usize, pub bytes: u64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String, pub language: String, pub bytes: u64, pub lines: Option<usize>,
    pub kind: String, pub role: String, pub confidence: u8, pub purpose: String,
    pub symbols: Vec<Symbol>, pub imports: Vec<ImportRef>, pub exports: Vec<String>,
    pub metrics: CodeMetrics, pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol { pub name: String, pub kind: String, pub line: Option<usize> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRef { pub raw: String, pub target: Option<String>, pub external: bool }
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMetrics {
    pub functions: usize, pub classes: usize, pub comments: usize, pub todo_count: usize,
    pub max_line_length: usize, pub estimated_complexity: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleReport { pub path: String, pub incoming: usize, pub outgoing: usize, pub role: String, pub centrality: f32 }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge { pub from: String, pub to: String, pub kind: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureInsight { pub title: String, pub detail: String, pub severity: String }

pub fn analyze_repository(root: &str) -> Result<RepoReport> {
    let root_path = PathBuf::from(root);
    if !root_path.is_dir() { return Err(anyhow!("Not a directory: {root}")); }
    let mut files = Vec::new();
    let mut directories = BTreeSet::new();
    let mut language_acc: BTreeMap<String,(usize,u64)> = BTreeMap::new();
    let mut frameworks = BTreeSet::new();
    let mut total_bytes = 0u64;
    let mut largest: Option<(u64,String)> = None;

    let walker = WalkBuilder::new(&root_path)
        .hidden(false).git_ignore(true).git_global(true).git_exclude(true)
        .filter_entry(|e| !ignored_dir(e.file_name().to_string_lossy().as_ref()))
        .build();
    for item in walker {
        let entry = item.context("walking repository")?;
        let path = entry.path();
        if path.is_dir() {
            if let Ok(rel) = path.strip_prefix(&root_path) {
                if !rel.as_os_str().is_empty() { directories.insert(norm(&rel.to_string_lossy())); }
            }
            continue;
        }
        let bytes = entry.metadata().context("reading file metadata")?.len();
        total_bytes += bytes;
        let rel = norm(&path.strip_prefix(&root_path).unwrap_or(path).to_string_lossy());
        if largest.as_ref().map(|v|v.0).unwrap_or(0) < bytes { largest = Some((bytes,rel.clone())); }
        let language = detect_language(path);
        let source = if bytes <= MAX_SOURCE_BYTES && is_text_language(&language) { fs::read_to_string(path).ok() } else { None };
        if let Some(s) = source.as_deref() { detect_frameworks(s,&language,&mut frameworks); }
        let report = analyze_file(&rel, bytes, &language, source.as_deref());
        let acc = language_acc.entry(language.clone()).or_insert((0,0)); acc.0 += 1; acc.1 += bytes;
        files.push(report);
    }
    files.sort_by(|a,b| a.path.cmp(&b.path));
    let edges = build_edges(&files);
    let modules = build_modules(&files,&edges);
    let architecture = infer_architecture(&files,&edges,&frameworks);
    let confidence = overall_confidence(&files,&frameworks);
    Ok(RepoReport {
        name: root_path.file_name().map(|x|x.to_string_lossy().to_string()).unwrap_or_else(||"Repository".into()),
        root: root_path.to_string_lossy().to_string(),
        summary: RepoSummary {
            files: files.len(), directories: directories.len(), source_files: files.iter().filter(|f|f.kind=="source").count(),
            total_bytes, largest_file: largest.map(|x|x.1), detected_frameworks: frameworks.iter().cloned().collect(), confidence,
        },
        languages: language_acc.into_iter().map(|(language,(files,bytes))| LanguageStat{language,files,bytes}).collect(),
        files,modules,edges,architecture,
        warnings: vec![
            "Static analysis infers intent from structure; it does not prove runtime behavior.".into(),
            "Generated/dependency directories are skipped to keep the scan focused and fast.".into(),
        ],
    })
}

fn ignored_dir(n:&str)->bool { matches!(n,".git"|"node_modules"|"target"|"dist"|"build"|"out"|"coverage"|".next"|".nuxt"|".svelte-kit"|".cache"|".turbo"|"vendor"|"Pods"|"DerivedData") }
fn norm(s:&str)->String { s.replace('\\',"/").trim_start_matches("./").to_string() }
fn is_text_language(l:&str)->bool { !matches!(l,"Other"|"Binary") }
fn detect_language(p:&Path)->String {
    match p.extension().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "rs"=>"Rust","js"|"mjs"|"cjs"=>"JavaScript","ts"|"mts"|"cts"=>"TypeScript","tsx"=>"TypeScript/JSX","jsx"=>"JavaScript/JSX",
        "py"=>"Python","go"=>"Go","java"=>"Java","kt"|"kts"=>"Kotlin","c"=>"C","cpp"|"cc"|"cxx"=>"C++","h"|"hpp"=>"C/C++ header",
        "cs"=>"C#","php"=>"PHP","rb"=>"Ruby","swift"=>"Swift","dart"=>"Dart","lua"=>"Lua","sh"|"bash"=>"Shell","sql"=>"SQL",
        "html"|"htm"=>"HTML","css"=>"CSS","scss"|"sass"=>"SCSS","json"=>"JSON","yaml"|"yml"=>"YAML","toml"=>"TOML","md"|"mdx"=>"Markdown",
        "xml"=>"XML","svg"=>"SVG","lock"=>"Lockfile",""=>filename_language(p),_=>"Other"
    }.into()
}
fn filename_language(p:&Path)->&str { match p.file_name().and_then(|x|x.to_str()).unwrap_or("") { "Dockerfile"=>"Dockerfile","Makefile"=>"Makefile","CMakeLists.txt"=>"CMake","Cargo.toml"=>"TOML","package.json"=>"JSON","requirements.txt"=>"Python requirements",_=>"Other" } }

fn classify_kind(path:&str, language:&str)->String {
    let n=Path::new(path).file_name().and_then(|x|x.to_str()).unwrap_or("").to_ascii_lowercase();
    if n.contains("readme")||n.contains("license") { return "documentation".into(); }
    if n.ends_with(".test.ts")||n.ends_with(".test.tsx")||n.ends_with(".spec.ts")||n.ends_with("_test.go")||n.starts_with("test_")||path.to_ascii_lowercase().contains("/tests/") { return "test".into(); }
    if n.contains("config")||path.to_ascii_lowercase().contains("/config/")||n==".env.example" { return "configuration".into(); }
    if matches!(language,"JSON"|"YAML"|"TOML"|"Lockfile"|"Markdown"|"HTML"|"CSS"|"SCSS"|"SVG"|"XML"|"Dockerfile"|"Makefile"|"CMake") { return "supporting".into(); }
    if language=="Other"||language=="Binary" { return "other".into(); }
    "source".into()
}

fn analyze_file(rel:&str,bytes:u64,language:&str,source:Option<&str>)->FileReport {
    let kind=classify_kind(rel,language);
    let src=source.unwrap_or("");
    let symbols=extract_symbols(src,language); let imports=extract_imports(src,language); let exports=extract_exports(src,language);
    let metrics=compute_metrics(src,language);
    let role=infer_role(rel,language,src,&imports,&exports);
    let confidence=role_confidence(rel,language,&role,&imports,src);
    let purpose=build_purpose(rel,language,&role,&symbols,&imports,&exports,&metrics);
    FileReport { path:rel.into(),language:language.into(),bytes,lines:source.map(|x|x.lines().count()),kind,role,confidence,purpose,symbols,imports,exports,warnings:file_warnings(rel,language,src,&metrics),metrics }
}
fn lower(s:&str)->String { s.to_ascii_lowercase() }
fn any_word(s:&str,words:&[&str])->bool { let l=lower(s); words.iter().any(|w|l.contains(&lower(w))) }
fn imported(imports:&[ImportRef],words:&[&str])->bool { imports.iter().any(|i|any_word(&i.raw,words)) }
fn infer_role(rel:&str,language:&str,src:&str,imports:&[ImportRef],exports:&[String])->String {
    let p=lower(rel); let n=lower(Path::new(rel).file_stem().and_then(|x|x.to_str()).unwrap_or(""));
    if p.contains("test") { return "Testing".into(); }
    if p.contains("migration") { return "Database migration".into(); }
    if p.contains("middleware")||n.contains("middleware") { return "Middleware".into(); }
    if p.contains("route")||p.contains("controller")||p.contains("handler") { return "API / request handling".into(); }
    if p.contains("auth")||any_word(src,&["login","logout","authenticate","authorization","jwt","session"]) { return "Authentication / authorization".into(); }
    if p.contains("database")||p.contains("db/")||n=="db"||imported(imports,&["postgres","mysql","sqlite","prisma","drizzle","sqlalchemy","mongodb","mongoose"]){ return "Database / persistence".into(); }
    if p.contains("component")||p.contains("ui/")||p.contains("views/")||language.contains("JSX")||any_word(src,&["useState","useEffect","<div","<button","<View"]){ return "UI / presentation".into(); }
    if p.contains("service")||n.ends_with("service") { return "Service / business logic".into(); }
    if p.contains("util")||p.contains("helper")||n.contains("util")||n.contains("helper") { return "Utility / helper".into(); }
    if p.contains("model")||p.contains("schema")||p.contains("entity")||n.contains("model")||n.contains("schema") { return "Data model / schema".into(); }
    if p.contains("store")||p.contains("state")||any_word(src,&["redux","zustand","createStore"]){ return "State management".into(); }
    if p.contains("cli")||any_word(src,&["clap","argparse","commander","process.argv"]){ return "CLI / command handling".into(); }
    if !exports.is_empty()&&imports.len()>=3 { return "Shared module / library".into(); }
    "General application logic".into()
}
fn role_confidence(rel:&str,language:&str,role:&str,imports:&[ImportRef],src:&str)->u8 {
    let p=lower(rel); let mut s=45u8; if !imports.is_empty(){s+=10} if !src.is_empty(){s+=10}
    if ["auth","test","route","controller","database","service","component"].iter().any(|x|p.contains(x)){s+=25}
    if role=="General application logic"{s=s.min(68)} if language=="Other"{s=25} s.min(95)
}
fn build_purpose(rel:&str,language:&str,role:&str,symbols:&[Symbol],imports:&[ImportRef],exports:&[String],metrics:&CodeMetrics)->String {
    let file=Path::new(rel).file_name().and_then(|x|x.to_str()).unwrap_or(rel);
    let noun=if role.contains("Testing"){"tests"}else if role.contains("UI"){"user-interface code"}else if role.contains("Database"){"database/persistence code"}else if role.contains("Authentication"){"authentication logic"}else if role.contains("API"){"request/API handling"}else if role.contains("Service"){"business/service logic"}else if role.contains("model"){"data modeling"}else{"application logic"};
    let mut s=format!("{file} appears to contain {noun}. It is written in {language}.");
    if !symbols.is_empty(){s.push_str(&format!(" It defines {} symbol(s), including {}.",symbols.len(),symbols.iter().take(4).map(|x|x.name.as_str()).collect::<Vec<_>>().join(", ")))}
    if !imports.is_empty(){s.push_str(&format!(" It imports {} module/package reference(s).",imports.len()))}
    if !exports.is_empty(){s.push_str(&format!(" It exposes {} export(s).",exports.len()))}
    if metrics.functions>0{s.push_str(&format!(" The code contains about {} function/method definition(s).",metrics.functions))}
    s
}
fn re(p:&str)->Regex { Regex::new(p).unwrap() }
fn extract_symbols(src:&str,language:&str)->Vec<Symbol>{
    let patterns:Vec<&str>=match language{
        "Rust"=>vec![r"\b(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",r"\b(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)",r"\b(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)",r"\b(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)"],
        "Python"=>vec![r"^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)",r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)"],
        "Go"=>vec![r"\bfunc\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)",r"\btype\s+([A-Za-z_][A-Za-z0-9_]*)\s+struct\b"],
        "Java"|"Kotlin"|"C#"=>vec![r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)",r"\binterface\s+([A-Za-z_][A-Za-z0-9_]*)",r"\b(?:public|private|protected|static|override|async|fun|void|int|String|boolean|bool|Task)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("],
        _=>vec![r"\bfunction\s+([A-Za-z_$][A-Za-z0-9_$]*)",r"\bclass\s+([A-Za-z_$][A-Za-z0-9_$]*)",r"\b(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][A-Za-z0-9_$]*)\s*=>"],
    };
    let mut seen=BTreeSet::new(); let mut out=Vec::new();
    for (i,line) in src.lines().enumerate(){ for p in &patterns{ if let Some(c)=re(p).captures(line){ if let Some(m)=c.get(1){ let name=m.as_str().to_string(); if seen.insert(name.clone()){ let kind=if line.contains("class "){"class"}else if line.contains("struct"){"struct"}else if line.contains("enum"){"enum"}else if line.contains("trait")||line.contains("interface"){"interface/trait"}else{"function/method"}; out.push(Symbol{name,kind:kind.into(),line:Some(i+1)}); }}}}}
    out
}
fn extract_imports(src:&str,language:&str)->Vec<ImportRef>{
    let mut out=Vec::new();
    for line in src.lines(){ let t=line.trim(); let raw=match language{ "Rust"=>after(t,"use ").or_else(||after(t,"mod ")),"Python"=>after(t,"import ").or_else(||after(t,"from ")),"Go"=>quoted(t),"Java"|"Kotlin"=>after(t,"import "),_=>js_import(t)}; if let Some(v)=raw{ let v=v.trim_matches(';').trim_matches(['"','\'']).to_string(); if v.is_empty(){continue}; let external=!v.starts_with('.')&&!v.starts_with('/')&&!v.starts_with("crate::")&&!v.starts_with("self::")&&!v.starts_with("super::"); out.push(ImportRef{raw:v.clone(),target:(!external).then_some(v),external}); }}
    out.sort_by(|a,b|a.raw.cmp(&b.raw)); out.dedup_by(|a,b|a.raw==b.raw); out
}
fn after(t:&str,p:&str)->Option<String>{t.strip_prefix(p).map(|x|x.split_whitespace().next().unwrap_or(x).to_string())}
fn quoted(t:&str)->Option<String>{t.split('"').nth(1).map(str::to_string)}
fn js_import(t:&str)->Option<String>{
    if let Some(x)=t.strip_prefix("import"){ if let Some(i)=x.find(" from "){return Some(x[i+6..].split_whitespace().next()?.trim_matches(['"','\'']).into())} return x.split_whitespace().next().map(|v|v.trim_matches(['"','\'']).into()) }
    if let Some(x)=t.strip_prefix("export"){ if let Some(i)=x.find(" from "){return Some(x[i+6..].split_whitespace().next()?.trim_matches(['"','\'']).into())} }
    if let Some(i)=t.find("require("){return t[i+8..].split(')').next().map(|v|v.trim_matches(['"','\'',' ']).into())}
    None
}
fn extract_exports(src:&str,language:&str)->Vec<String>{
    let mut out=Vec::new();
    for t in src.lines().map(str::trim){
        if matches!(language,"JavaScript"|"TypeScript"|"JavaScript/JSX"|"TypeScript/JSX") && t.starts_with("export "){
            if t.starts_with("export default"){out.push("default".into())}
            for key in ["function ","class ","const ","let ","var "]{if let Some(i)=t.find(key){if let Some(n)=t[i+key.len()..].split(|c:char|!c.is_alphanumeric()&&c!='_'&&c!='$').next(){if !n.is_empty(){out.push(n.into())}}}}
        } else if language=="Rust" && t.starts_with("pub "){
            if let Some(n)=t.split_whitespace().nth(2){let n=n.trim_matches(|c:char|!c.is_alphanumeric()&&c!='_');if !n.is_empty(){out.push(n.into())}}
        }
    }
    out.sort();out.dedup();out
}
fn compute_metrics(src:&str,language:&str)->CodeMetrics{ let comments=src.lines().filter(|l|{let t=l.trim();t.starts_with("//")||t.starts_with('#')||t.starts_with("/*")||t.starts_with('*')||t.starts_with("<!--")}).count(); let todo_count=src.lines().filter(|l|{let t=lower(l);t.contains("todo")||t.contains("fixme")||t.contains("hack")}).count(); let max_line_length=src.lines().map(|l|l.chars().count()).max().unwrap_or(0); let symbols=extract_symbols(src,language); let functions=symbols.iter().filter(|s|s.kind=="function/method").count(); let classes=symbols.iter().filter(|s|s.kind=="class"||s.kind=="struct").count(); let estimated_complexity=src.matches(" if ").count()+src.matches(" if(").count()+src.matches(" for ").count()+src.matches(" while ").count()+src.matches(" match ").count()+src.matches(" case ").count()+src.matches("&&").count()+src.matches("||").count(); CodeMetrics{functions,classes,comments,todo_count,max_line_length,estimated_complexity} }
fn file_warnings(path:&str,language:&str,src:&str,m:&CodeMetrics)->Vec<String>{ let mut w=Vec::new(); if m.max_line_length>180{w.push(format!("Contains very long lines (up to {} chars).",m.max_line_length));} if m.todo_count>0{w.push(format!("Contains {} TODO/FIXME/HACK marker(s).",m.todo_count));} let l=lower(src); if l.contains("api_key =")||l.contains("password =")||l.contains("secret ="){w.push("Possible hard-coded secret-like assignment; verify manually.".into())} if language=="SQL"&&l.contains("select *"){w.push("Uses SELECT *; explicit columns may be clearer.".into())} if path.contains(".env")&&!path.ends_with(".example"){w.push("Environment file may contain sensitive configuration.".into())} w }
fn detect_frameworks(src:&str,language:&str,f:&mut BTreeSet<String>){let s=lower(src); if matches!(language,"JavaScript"|"TypeScript"|"JavaScript/JSX"|"TypeScript/JSX"){for (needle,name) in [("next/","Next.js"),("from 'react'","React"),("from \"react\"","React"),("express","Express"),("@nestjs/","NestJS"),("fastify","Fastify"),("vue","Vue"),("svelte","Svelte")]{if s.contains(needle){f.insert(name.into());}}} if language=="Rust"{for (needle,name) in [("tauri::","Tauri"),("actix_web","Actix Web"),("axum","Axum"),("rocket","Rocket")]{if s.contains(needle){f.insert(name.into());}}} if language=="Python"{for (needle,name) in [("django","Django"),("fastapi","FastAPI"),("flask","Flask")]{if s.contains(needle){f.insert(name.into());}}}}

fn build_edges(files:&[FileReport])->Vec<DependencyEdge>{
    let known:HashMap<String,String>=files.iter().map(|f|(norm(&f.path),f.path.clone())).collect(); let mut out=Vec::new();
    for f in files.iter().filter(|f|f.kind=="source"){for imp in &f.imports{if let Some(raw)=&imp.target{if let Some(target)=resolve_local(&f.path,raw,&known){if target!=f.path{out.push(DependencyEdge{from:f.path.clone(),to:target,kind:"imports".into()})}}}}}
    out.sort_by(|a,b|a.from.cmp(&b.from).then(a.to.cmp(&b.to)));out.dedup_by(|a,b|a.from==b.from&&a.to==b.to);out
}
fn resolve_local(from:&str,raw:&str,known:&HashMap<String,String>)->Option<String>{let base=Path::new(from).parent().unwrap_or(Path::new(""));let c=norm(&base.join(raw).to_string_lossy());let vars=[c.clone(),format!("{c}.ts"),format!("{c}.tsx"),format!("{c}.js"),format!("{c}.jsx"),format!("{c}.rs"),format!("{c}/index.ts"),format!("{c}/index.tsx"),format!("{c}/index.js"),format!("{c}/mod.rs")];vars.into_iter().find_map(|v|known.get(&v).cloned())}
fn build_modules(files:&[FileReport],edges:&[DependencyEdge])->Vec<ModuleReport>{let mut i:HashMap<&str,usize>=HashMap::new();let mut o:HashMap<&str,usize>=HashMap::new();for e in edges{*o.entry(&e.from).or_default()+=1;*i.entry(&e.to).or_default()+=1}files.iter().filter(|f|f.kind=="source").map(|f|{let incoming=*i.get(f.path.as_str()).unwrap_or(&0);let outgoing=*o.get(f.path.as_str()).unwrap_or(&0);ModuleReport{path:f.path.clone(),incoming,outgoing,role:f.role.clone(),centrality:((incoming+outgoing)as f32).sqrt()}}).collect()}
fn infer_architecture(files:&[FileReport],edges:&[DependencyEdge],frameworks:&BTreeSet<String>)->Vec<ArchitectureInsight>{let mut v=Vec::new();if !frameworks.is_empty(){v.push(ArchitectureInsight{title:"Detected technology stack".into(),detail:format!("The repository appears to use {}.",frameworks.iter().cloned().collect::<Vec<_>>().join(", ")),severity:"info".into()})}let hubs=files.iter().filter(|f|edges.iter().filter(|e|e.from==f.path||e.to==f.path).count()>=8).count();if hubs>0{v.push(ArchitectureInsight{title:"Central modules".into(),detail:format!("{hubs} source file(s) have unusually high local connectivity and may act as shared hubs."),severity:"info".into()})}let large=files.iter().filter(|f|f.lines.unwrap_or(0)>500).count();if large>0{v.push(ArchitectureInsight{title:"Large source files".into(),detail:format!("{large} source file(s) exceed 500 lines; they may combine multiple responsibilities."),severity:"warning".into()})}let todo=files.iter().map(|f|f.metrics.todo_count).sum::<usize>();if todo>0{v.push(ArchitectureInsight{title:"Unresolved work markers".into(),detail:format!("Found {todo} TODO/FIXME/HACK marker(s)."),severity:"warning".into()})}if edges.is_empty()&&files.len()>3{v.push(ArchitectureInsight{title:"Dependency graph is incomplete".into(),detail:"The scanner found few resolvable local imports. Language-specific AST parsing and package-aware resolution are planned for later passes.".into(),severity:"warning".into()})}v}
fn overall_confidence(files:&[FileReport],frameworks:&BTreeSet<String>)->u8{if files.is_empty(){0}else{let avg=files.iter().map(|f|f.confidence as u32).sum::<u32>()/files.len() as u32;(avg as u8+if frameworks.is_empty(){0}else{8}).min(96)}}
