use anyhow::{Context, Result, bail};
use lily2vec::{Model, vnni_available};
use std::{
    hint::black_box,
    io::{self, BufRead, Write},
    time::Instant,
};

fn classify(model: &Model, text: &str, detail: bool) -> Result<()> {
    let start = Instant::now();
    let query = model.encode(text)?;
    let scores = model.score_vnni(&query)?;
    let token = &model.labels[model.winner(&scores)];
    let elapsed = start.elapsed();
    println!("{token}");
    io::stdout().flush()?;
    if detail {
        let mut ranked: Vec<_> = model.labels.iter().zip(&scores).collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1));
        eprintln!(
            "backend=avx-vnni instruction=vpdpbusd active_features={} dimensions={} selection_us={:.3}",
            query.iter().filter(|&&v| v != 0).count(),
            model.dimensions,
            elapsed.as_secs_f64() * 1e6
        );
        for (label, score) in ranked.iter().take(5) {
            eprintln!("{label:12} {score}");
        }
    }
    Ok(())
}

fn bench(model: &Model, iterations: u32) -> Result<()> {
    if iterations == 0 {
        bail!("iterations must be positive");
    }
    let inputs = [
        "hi",
        "i dont feel well",
        "i love you",
        "NO",
        "yes that will do",
        "not angry, just explaining",
        "what if we tried this",
    ];
    let queries: Vec<_> = inputs
        .iter()
        .map(|t| model.encode(t))
        .collect::<Result<_>>()?;
    for q in &queries {
        assert_eq!(model.score_vnni(q)?, model.score_reference(q));
    }
    // Both paths include score allocation, bias and winner, but exclude feature extraction.
    for vnni in [false, true] {
        let start = Instant::now();
        let mut checksum = 0_u64;
        for i in 0..iterations {
            let q = black_box(&queries[i as usize % queries.len()]);
            let scores = if vnni {
                model.score_vnni(q)?
            } else {
                model.score_reference(q)
            };
            checksum = checksum.wrapping_add(black_box(scores[model.winner(&scores)]) as u64);
            black_box(&scores);
        }
        eprintln!(
            "backend={} scoring_ns={:.1} iterations={iterations} checksum={checksum}",
            if vnni { "avx-vnni" } else { "reference" },
            start.elapsed().as_secs_f64() * 1e9 / f64::from(iterations)
        );
    }
    let start = Instant::now();
    for i in 0..iterations {
        let query = model.encode(black_box(inputs[i as usize % inputs.len()]))?;
        let scores = model.score_vnni(&query)?;
        black_box(model.winner(&scores));
    }
    eprintln!(
        "backend=avx-vnni whole_selector_ns={:.1} (features + scoring + winner; excludes terminal IO)",
        start.elapsed().as_secs_f64() * 1e9 / f64::from(iterations)
    );
    Ok(())
}

fn main() -> Result<()> {
    let mut guide = include_str!("../vibes.json").to_owned();
    let mut detail = false;
    let mut benchmark = None;
    let mut words = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "lily2vec [--scores] [--guide FILE] [--bench ITERATIONS] [TEXT...]\n\nCPU AVX-VNNI vibe classifier. No downloads or pretrained model.\nWithout TEXT, type one input per line; Ctrl-D exits. Stdout is one vibe token.\n--scores prints integer scores and backend to stderr.\n--bench compares VNNI scoring with its integer reference and times the whole selector.\nAVX-VNNI hardware and OS vector state are required. No fallback backend.\nThe previous Potion experiment is available with: cargo run --release --features potion --bin lily2vec-potion"
                );
                return Ok(());
            }
            "--scores" => detail = true,
            "--guide" => {
                guide = std::fs::read_to_string(args.next().context("--guide requires a file")?)?
            }
            "--bench" => {
                benchmark = Some(
                    args.next()
                        .context("--bench requires iterations")?
                        .parse::<u32>()?,
                )
            }
            "--" => {
                words.extend(args);
                break;
            }
            _ if arg.starts_with('-') => bail!("unknown option {arg}"),
            _ => words.push(arg),
        }
    }
    if !vnni_available() {
        bail!("AVX-VNNI + AVX2 unavailable; no substitute scoring backend is used");
    }
    let model = Model::load(&guide)?;
    if let Some(n) = benchmark {
        return bench(&model, n);
    }
    if !words.is_empty() {
        return classify(&model, &words.join(" "), detail);
    }
    for line in io::stdin().lock().lines() {
        let line = line?;
        if !line.trim().is_empty() {
            classify(&model, &line, detail)?;
        }
    }
    Ok(())
}
