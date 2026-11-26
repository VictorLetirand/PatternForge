//! Windows : l'icône est inscrite dans l'exécutable, d'où l'Explorateur et les fichiers `.pforge`
//! associés la tirent. Sans effet ailleurs.
fn main() {
    println!("cargo::rerun-if-changed=../../assets/icon/icon.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo::warning=icône non inscrite : {e}");
        }
    }
}
