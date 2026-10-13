fn main(){
    use std::{env,path::PathBuf,process::Command};
    println!("cargo:rerun-if-changed=../../assets/OutSwitch.ico");
    println!("cargo:rerun-if-changed=../../assets/OutSwitch.rc");
    let mut root=PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());root.pop();root.pop();root.push("assets");
    let msvc=env::var("CARGO_CFG_TARGET_ENV").unwrap()=="msvc";
    let output=PathBuf::from(env::var("OUT_DIR").unwrap()).join(if msvc{"icon.res"}else{"icon.o"});
    let status=if msvc{
        let rc=env::var_os("RC").map(PathBuf::from).or_else(||{
            let base=PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
            let mut versions:Vec<_>=std::fs::read_dir(base).ok()?.filter_map(Result::ok).map(|e|e.path()).collect();versions.sort();
            versions.into_iter().rev().map(|p|p.join("x64/rc.exe")).find(|p|p.is_file())
        }).unwrap_or_else(||"rc.exe".into());
        Command::new(rc).arg("/nologo").arg(format!("/fo{}",output.display())).arg("OutSwitch.rc").current_dir(&root).status()
    }
    else{Command::new(env::var("WINDRES").unwrap_or_else(|_|"windres".into())).args(["-i","OutSwitch.rc","-O","coff","-o"]).arg(&output).current_dir(&root).status()}.expect("Windows resource compiler is required");
    assert!(status.success(),"Windows application icon compilation failed");println!("cargo:rustc-link-arg={}",output.display());
}
