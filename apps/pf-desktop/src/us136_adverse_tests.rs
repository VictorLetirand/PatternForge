//! US-13.6 / US-13.7 : campagne adverse à graine fixe (300 séquences). Oracle écrit à part du code
//! testé : les règles de validité sont recopiées ici en dur (bornes de l'énoncé), pas lues dans
//! `projection.rs` ni `direct_print.rs`.

use crate::direct_print::{
    print_pdf, PrintError, PrintRequest, PrinterInfo, Spooler, MAX_COPIES, SCALE_PERCENT,
};
use crate::projection::{self, ProjectionError, ProjectionSettings};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn field(&mut self, lo: f64, hi: f64) -> f64 {
        if self.below(6) == 0 {
            self.f()
        } else {
            lo + (hi - lo) * (self.below(1_000_001) as f64) / 1_000_000.0
        }
    }
    fn f(&mut self) -> f64 {
        const WILD: [f64; 9] = [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -1.0,
            1e300,
            -1e-300,
            49.999,
            20_000.001,
        ];
        match self.below(4) {
            0 => WILD[self.below(WILD.len() as u64) as usize],
            1 => (self.below(40_000) as f64) / 2.0,
            _ => (self.below(2_000_000) as f64) / 1000.0,
        }
    }
}

fn oracle(s: &ProjectionSettings) -> Result<(), &'static str> {
    let ok = |v: f64, lo: f64, hi: f64| v.is_finite() && v >= lo && v <= hi;
    if !ok(s.screen_width_mm, 50.0, 20_000.0) {
        return Err("width");
    }
    if !ok(s.grid_step_mm, 1.0, 500.0) {
        return Err("grid");
    }
    if !ok(s.stroke_mm, 0.1, 20.0) {
        return Err("stroke");
    }
    Ok(())
}

#[test]
fn campagne_projection_150_sequences() {
    let mut rng = Rng(0x1307_2026);
    let (mut valid, mut invalid, mut px_bad, mut mirrored) = (0, 0, 0, 0);
    for _ in 0..150 {
        let s = ProjectionSettings {
            screen_width_mm: rng.field(50.0, 20_000.0),
            grid_step_mm: rng.field(1.0, 500.0),
            stroke_mm: rng.field(0.1, 20.0),
            mirror_h: rng.below(2) == 0,
            mirror_v: rng.below(2) == 0,
        };
        let px = [1920.0, 1280.0, 0.0, -3.0, f64::NAN, 1e9][rng.below(6) as usize];
        let got = projection::pixels_per_mm(&s, px);
        match (oracle(&s), got) {
            (Err(which), Err(e)) => {
                invalid += 1;
                let name = match e {
                    ProjectionError::ScreenWidth(_) => "width",
                    ProjectionError::GridStep(_) => "grid",
                    ProjectionError::Stroke(_) => "stroke",
                    ProjectionError::ScreenPixels(_) => "pixels",
                };
                assert_eq!(name, which, "{s:?}");
            }
            (Ok(()), Err(ProjectionError::ScreenPixels(_))) => {
                assert!(!(px.is_finite() && px >= 1.0), "{px}");
                px_bad += 1;
            }
            (Ok(()), Ok(k)) => {
                valid += 1;
                assert_eq!(k, px / s.screen_width_mm);
                let hundred = 100.0 * px / s.screen_width_mm;
                assert!(
                    (projection::mm_to_px(100.0, k) - hundred).abs() <= 1e-9 * hundred.max(1.0)
                );
                let back = projection::px_to_mm(projection::mm_to_px(37.5, k), k);
                assert!((back - 37.5).abs() < 1e-9);
                assert!(projection::stroke_px(&s, k) >= 1.0);
                if s.mirror_h || s.mirror_v {
                    mirrored += 1;
                    assert_eq!(projection::mirror(10.0, 100.0, true), 90.0);
                }
            }
            (a, b) => panic!("oracle {a:?} contre {b:?} pour {s:?}"),
        }
    }
    assert!(valid > 10 && invalid > 20 && px_bad > 3 && mirrored > 5);
}

struct Wild {
    list: Result<Vec<PrinterInfo>, PrintError>,
    fail_submit: bool,
    jobs: RefCell<Vec<PrintRequest>>,
}

impl Spooler for Wild {
    fn printers(&self) -> Result<Vec<PrinterInfo>, PrintError> {
        self.list.clone()
    }
    fn submit(&self, job: &PrintRequest) -> Result<(), PrintError> {
        assert!(job.file.exists(), "fichier présent pendant l'envoi");
        if self.fail_submit {
            return Err(PrintError::Spooler("refus".into()));
        }
        self.jobs.borrow_mut().push(job.clone());
        Ok(())
    }
}

#[test]
fn campagne_impression_150_sequences() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("pf-adverse-print-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut rng = Rng(0x1306_2026);
    let (mut sent, mut none, mut unknown, mut copies_bad, mut cancelled, mut refused, mut listerr) =
        (0, 0, 0, 0, 0, 0, 0);
    for _ in 0..150 {
        let n = rng.below(4) as usize;
        let names = ["A", "B", "C"];
        let printers: Vec<PrinterInfo> = (0..n)
            .map(|i| PrinterInfo {
                name: names[i].to_string(),
                is_default: rng.below(3) == 0,
            })
            .collect();
        let list_fails = rng.below(10) == 0;
        let sp = Wild {
            list: if list_fails {
                Err(PrintError::Spooler("liste".into()))
            } else {
                Ok(printers.clone())
            },
            fail_submit: rng.below(6) == 0,
            jobs: RefCell::new(Vec::new()),
        };
        let wanted = match rng.below(3) {
            0 => None,
            1 => Some("B"),
            _ => Some("Z"),
        };
        let copies = rng.below(130) as u32;
        let cancel = AtomicBool::new(rng.below(8) == 0);
        let pdf = vec![b'x'; rng.below(5000) as usize];
        let res = print_pdf(&sp, &pdf, wanted, copies, &dir, &cancel);

        let expected: Result<String, PrintError> = if copies == 0 || copies > 99 {
            Err(PrintError::BadCopies(copies))
        } else if list_fails {
            Err(PrintError::Spooler("liste".into()))
        } else if printers.is_empty() {
            Err(PrintError::NoPrinter)
        } else if let Some(w) = wanted {
            if printers.iter().any(|p| p.name == w) {
                Ok(w.to_string())
            } else {
                Err(PrintError::UnknownPrinter(w.into()))
            }
        } else {
            Ok(printers
                .iter()
                .find(|p| p.is_default)
                .unwrap_or(&printers[0])
                .name
                .clone())
        };
        let expected = match expected {
            Ok(_) if cancel.load(Ordering::SeqCst) => Err(PrintError::Cancelled),
            Ok(name) if sp.fail_submit => {
                let _ = name;
                Err(PrintError::Spooler("refus".into()))
            }
            other => other,
        };
        match (&expected, &res) {
            (Ok(name), Ok(r)) => {
                sent += 1;
                assert_eq!(&r.printer, name);
                assert!(r.temp_removed);
                assert_eq!(r.bytes, pdf.len());
                let jobs = sp.jobs.borrow();
                assert_eq!(jobs.len(), 1);
                assert_eq!(jobs[0].scale_percent, SCALE_PERCENT);
                assert_eq!(jobs[0].scale_percent, 100);
                assert!(jobs[0].copies >= 1 && jobs[0].copies <= MAX_COPIES);
            }
            (Err(e), Err(g)) => {
                assert_eq!(e, g);
                assert!(sp.jobs.borrow().is_empty());
                match e {
                    PrintError::NoPrinter => none += 1,
                    PrintError::UnknownPrinter(_) => unknown += 1,
                    PrintError::BadCopies(_) => copies_bad += 1,
                    PrintError::Cancelled => cancelled += 1,
                    PrintError::Spooler(m) if m == "refus" => refused += 1,
                    PrintError::Spooler(_) => listerr += 1,
                    PrintError::Io(_) => panic!("E/S inattendue"),
                }
            }
            (a, b) => panic!("oracle {a:?} contre {b:?}"),
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "temporaire");
    }
    assert!(sent > 15 && none > 5 && unknown > 5 && copies_bad > 10);
    assert!(cancelled > 3 && refused > 3 && listerr > 3);
    let _ = std::fs::remove_dir_all(dir);
}
