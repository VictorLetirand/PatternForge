//! US-13.6 : impression directe (File > Imprimer...).
//!
//! Approche retenue : le PDF **existant** (`pf_export::pdf::export_pdf_lang`, même contenu, même
//! échelle 1:1, étalon de 100 mm compris, mêmes octets que « Exporter PDF ») est écrit dans un
//! fichier temporaire, puis confié au système d'impression de Windows par le verbe `PrintTo` de
//! l'association de fichier, sans fenêtre ni boîte bloquante. Le rendu direct vers le spouleur
//! (GDI) demanderait une pile graphique (bindings Win32) et un second moteur de mise en page qui
//! pourrait diverger de l'export : rejeté. Tout passe par le trait `Spooler`, remplacé par un
//! faux dans les tests (aucune imprimante, aucun processus lancé).
//!
//! L'échelle est **100 % imposée** : `PrintRequest::scale_percent` vaut toujours 100 et aucune
//! option « ajuster à la page » n'existe. Ce que l'application associée au PDF en fait ne peut
//! pas être vérifié sans papier : **[HUMAIN]**, précision « calculée, non mesurée sur papier ».

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const SCALE_PERCENT: u32 = 100;
pub const MAX_COPIES: u32 = 99;
const TEMP_PREFIX: &str = "pforge-print-";
#[cfg(windows)]
const REAL_KEEP_SECS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrintError {
    NoPrinter,
    UnknownPrinter(String),
    BadCopies(u32),
    Cancelled,
    Io(String),
    Spooler(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintRequest {
    pub file: PathBuf,
    pub printer: String,
    pub copies: u32,
    /// Toujours 100 : jamais « ajuster à la page ».
    pub scale_percent: u32,
}

pub trait Spooler {
    fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError>;
    fn submit(&self, job: &PrintRequest) -> Result<(), PrintError>;
    /// Vrai si le spouleur supprime lui-même le fichier plus tard (l'application associée le lit
    /// encore après `submit`).
    fn owns_cleanup(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintReport {
    pub printer: String,
    pub bytes: usize,
    pub copies: u32,
    pub temp_removed: bool,
}

/// Imprimante choisie : celle demandée (refus si inconnue), sinon la valeur par défaut du système,
/// sinon la première. Aucune imprimante = refus nommé.
pub fn choose_printer(list: &[PrinterInfo], wanted: Option<&str>) -> Result<String, PrintError> {
    if list.is_empty() {
        return Err(PrintError::NoPrinter);
    }
    if let Some(w) = wanted {
        return list
            .iter()
            .find(|p| p.name == w)
            .map(|p| p.name.clone())
            .ok_or_else(|| PrintError::UnknownPrinter(w.to_string()));
    }
    Ok(list
        .iter()
        .find(|p| p.is_default)
        .unwrap_or(&list[0])
        .name
        .clone())
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_name() -> String {
    format!(
        "{TEMP_PREFIX}{}-{}.pdf",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Imprime les octets d'un PDF déjà généré. Annulable : `cancel` est lu avant l'écriture et avant
/// l'envoi ; le fichier temporaire n'est jamais laissé (sauf si le spouleur en reprend la charge).
pub fn print_pdf(
    spooler: &dyn Spooler,
    pdf: &[u8],
    printer: Option<&str>,
    copies: u32,
    temp_dir: &Path,
    cancel: &AtomicBool,
) -> Result<PrintReport, PrintError> {
    if copies == 0 || copies > MAX_COPIES {
        return Err(PrintError::BadCopies(copies));
    }
    let name = choose_printer(&spooler.printers()?, printer)?;
    if cancel.load(Ordering::SeqCst) {
        return Err(PrintError::Cancelled);
    }
    let file = temp_dir.join(temp_name());
    std::fs::write(&file, pdf).map_err(|e| PrintError::Io(e.to_string()))?;
    let result = if cancel.load(Ordering::SeqCst) {
        Err(PrintError::Cancelled)
    } else {
        spooler.submit(&PrintRequest {
            file: file.clone(),
            printer: name.clone(),
            copies,
            scale_percent: SCALE_PERCENT,
        })
    };
    let temp_removed = if spooler.owns_cleanup() && result.is_ok() {
        false
    } else {
        std::fs::remove_file(&file).is_ok()
    };
    result.map(|()| PrintReport {
        printer: name,
        bytes: pdf.len(),
        copies,
        temp_removed,
    })
}

/// Supprime les fichiers d'impression oubliés (arrêt brutal) : appelé au démarrage de la fenêtre.
pub fn sweep_stale(temp_dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(temp_dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(TEMP_PREFIX)
                && n.ends_with(".pdf")
                && std::fs::remove_file(e.path()).is_ok()
        })
        .count()
}

/// « nom|True » par ligne (sortie de la requête système).
#[cfg(any(windows, test))]
pub fn parse_printers(text: &str) -> Vec<PrinterInfo> {
    text.lines()
        .filter_map(|l| {
            let (name, def) = l.trim().rsplit_once('|')?;
            let name = name.trim();
            (!name.is_empty()).then(|| PrinterInfo {
                name: name.to_string(),
                is_default: def.trim().eq_ignore_ascii_case("true"),
            })
        })
        .collect()
}

/// Spouleur réel : PowerShell (Windows). Le nom du fichier et celui de l'imprimante passent par
/// l'environnement, jamais dans la ligne de commande (pas d'injection).
pub struct SystemSpooler;

#[cfg(windows)]
fn powershell(script: &str, envs: &[(&str, &str)]) -> Result<String, PrintError> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .map_err(|e| PrintError::Spooler(e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(PrintError::Spooler(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

impl Spooler for SystemSpooler {
    #[cfg(windows)]
    fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
        let text = powershell(
            "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
             Get-CimInstance Win32_Printer | ForEach-Object { $_.Name + '|' + $_.Default }",
            &[],
        )?;
        Ok(parse_printers(&text))
    }

    #[cfg(not(windows))]
    fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
        Err(PrintError::Spooler("windows only".into()))
    }

    #[cfg(windows)]
    fn submit(&self, job: &PrintRequest) -> Result<(), PrintError> {
        if job.printer.contains('"') {
            return Err(PrintError::UnknownPrinter(job.printer.clone()));
        }
        let file = job.file.to_string_lossy().into_owned();
        let script = "for ($i=0; $i -lt [int]$env:PF_PRINT_COPIES; $i++) { \
             Start-Process -FilePath $env:PF_PRINT_FILE -Verb PrintTo \
             -ArgumentList ('\"' + $env:PF_PRINT_PRINTER + '\"') -WindowStyle Hidden }";
        powershell(
            script,
            &[
                ("PF_PRINT_FILE", &file),
                ("PF_PRINT_PRINTER", &job.printer),
                ("PF_PRINT_COPIES", &job.copies.to_string()),
            ],
        )?;
        let path = job.file.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(REAL_KEEP_SECS));
            let _ = std::fs::remove_file(path);
        });
        Ok(())
    }

    #[cfg(not(windows))]
    fn submit(&self, _job: &PrintRequest) -> Result<(), PrintError> {
        Err(PrintError::Spooler("windows only".into()))
    }

    fn owns_cleanup(&self) -> bool {
        true
    }
}

/// Spouleur de démonstration (étape de capture) : deux imprimantes fictives, aucun envoi réel.
pub struct DemoSpooler;

impl Spooler for DemoSpooler {
    fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
        Ok(vec![
            PrinterInfo {
                name: "Printer A4".into(),
                is_default: true,
            },
            PrinterInfo {
                name: "Plotter A0".into(),
                is_default: false,
            },
        ])
    }
    fn submit(&self, _job: &PrintRequest) -> Result<(), PrintError> {
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Faux spouleur : enregistre les travaux, ne lance aucun processus.
    pub struct FakeSpooler {
        pub list: Vec<PrinterInfo>,
        pub fail: Option<PrintError>,
        pub jobs: RefCell<Vec<(PrintRequest, Vec<u8>)>>,
    }

    impl FakeSpooler {
        pub fn with(names: &[(&str, bool)]) -> FakeSpooler {
            FakeSpooler {
                list: names
                    .iter()
                    .map(|(n, d)| PrinterInfo {
                        name: (*n).to_string(),
                        is_default: *d,
                    })
                    .collect(),
                fail: None,
                jobs: RefCell::new(Vec::new()),
            }
        }
    }

    impl Spooler for FakeSpooler {
        fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
            Ok(self.list.clone())
        }
        fn submit(&self, job: &PrintRequest) -> Result<(), PrintError> {
            if let Some(e) = &self.fail {
                return Err(e.clone());
            }
            let bytes = std::fs::read(&job.file).map_err(|e| PrintError::Io(e.to_string()))?;
            self.jobs.borrow_mut().push((job.clone(), bytes));
            Ok(())
        }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pf-direct-print-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn files(d: &Path) -> usize {
        std::fs::read_dir(d).unwrap().count()
    }

    #[test]
    fn le_travail_porte_les_memes_octets_a_100_pour_cent_et_le_temporaire_disparait() {
        let d = dir("ok");
        let sp = FakeSpooler::with(&[("A", false), ("B", true)]);
        let pdf = b"%PDF-1.4 essai";
        let r = print_pdf(&sp, pdf, None, 2, &d, &AtomicBool::new(false)).unwrap();
        assert_eq!(r.printer, "B");
        assert!(r.temp_removed);
        assert_eq!(files(&d), 0);
        let jobs = sp.jobs.borrow();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].0.scale_percent, 100);
        assert_eq!(jobs[0].0.copies, 2);
        assert_eq!(jobs[0].1, pdf);
    }

    #[test]
    fn aucune_imprimante_est_un_refus_nomme_sans_fichier() {
        let d = dir("none");
        let sp = FakeSpooler::with(&[]);
        let e = print_pdf(&sp, b"x", None, 1, &d, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(e, PrintError::NoPrinter);
        assert_eq!(files(&d), 0);
    }

    #[test]
    fn imprimante_inconnue_refusee_et_choix_explicite_respecte() {
        let sp = FakeSpooler::with(&[("A", true), ("B", false)]);
        assert_eq!(choose_printer(&sp.list, Some("B")).unwrap(), "B");
        assert_eq!(
            choose_printer(&sp.list, Some("Z")),
            Err(PrintError::UnknownPrinter("Z".into()))
        );
        let sans_defaut = FakeSpooler::with(&[("A", false), ("B", false)]);
        assert_eq!(choose_printer(&sans_defaut.list, None).unwrap(), "A");
    }

    #[test]
    fn annulation_avant_envoi_ne_laisse_rien_et_n_envoie_rien() {
        let d = dir("cancel");
        let sp = FakeSpooler::with(&[("A", true)]);
        let e = print_pdf(&sp, b"x", None, 1, &d, &AtomicBool::new(true)).unwrap_err();
        assert_eq!(e, PrintError::Cancelled);
        assert!(sp.jobs.borrow().is_empty());
        assert_eq!(files(&d), 0);
    }

    #[test]
    fn echec_du_spouleur_supprime_quand_meme_le_temporaire() {
        let d = dir("fail");
        let mut sp = FakeSpooler::with(&[("A", true)]);
        sp.fail = Some(PrintError::Spooler("boom".into()));
        let e = print_pdf(&sp, b"x", None, 1, &d, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(e, PrintError::Spooler("boom".into()));
        assert_eq!(files(&d), 0);
    }

    #[test]
    fn copies_hors_bornes_refusees() {
        let d = dir("copies");
        let sp = FakeSpooler::with(&[("A", true)]);
        for c in [0, 100, u32::MAX] {
            assert_eq!(
                print_pdf(&sp, b"x", None, c, &d, &AtomicBool::new(false)),
                Err(PrintError::BadCopies(c))
            );
        }
        assert!(print_pdf(&sp, b"x", None, 99, &d, &AtomicBool::new(false)).is_ok());
    }

    #[test]
    fn la_liste_systeme_est_lue_ligne_a_ligne() {
        let l = parse_printers(
            "Microsoft Print to PDF|False\r\nHP | LaserJet|True\r\n\r\n|True\r\nsans-barre\r\n",
        );
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].name, "Microsoft Print to PDF");
        assert!(!l[0].is_default);
        assert_eq!(l[1].name, "HP | LaserJet");
        assert!(l[1].is_default);
    }

    #[test]
    fn le_balayage_ne_supprime_que_les_fichiers_d_impression() {
        let d = dir("sweep");
        std::fs::write(d.join("pforge-print-1-1.pdf"), b"x").unwrap();
        std::fs::write(d.join("pforge-print-9-2.pdf"), b"x").unwrap();
        std::fs::write(d.join("autre.pdf"), b"x").unwrap();
        assert_eq!(sweep_stale(&d), 2);
        assert_eq!(files(&d), 1);
    }

    #[test]
    fn un_spouleur_qui_reprend_le_fichier_le_conserve() {
        struct Keeps;
        impl Spooler for Keeps {
            fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
                Ok(vec![PrinterInfo {
                    name: "A".into(),
                    is_default: true,
                }])
            }
            fn submit(&self, _: &PrintRequest) -> Result<(), PrintError> {
                Ok(())
            }
            fn owns_cleanup(&self) -> bool {
                true
            }
        }
        let d = dir("keeps");
        let r = print_pdf(&Keeps, b"x", None, 1, &d, &AtomicBool::new(false)).unwrap();
        assert!(!r.temp_removed);
        assert_eq!(files(&d), 1);
    }
}
