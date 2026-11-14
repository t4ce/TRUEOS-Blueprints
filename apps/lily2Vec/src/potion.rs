//! Original pretrained embedding comparison, kept behind the `potion` feature.
use anyhow::{Context, Result, bail};
use model2vec_rs::model::StaticModel;
use serde::Deserialize;
use std::io::{self, BufRead};
use std::time::Instant;
#[derive(Deserialize)]
struct Emotion {
    token: String,
    examples: Vec<String>,
}
fn main() -> Result<()> {
    let mut model = "minishlab/potion-base-8M".to_owned();
    let mut catalog = include_str!("../emotions.json").to_owned();
    let mut scores = false;
    let mut words = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model" => model = args.next().context("--model requires a path or ID")?,
            "--catalog" => {
                catalog =
                    std::fs::read_to_string(args.next().context("--catalog requires a file")?)?
            }
            "--scores" => scores = true,
            "--help" | "-h" => {
                println!(
                    "lily2vec-potion [--model ID_OR_PATH] [--catalog FILE] [--scores] [TEXT...]"
                );
                return Ok(());
            }
            "--" => {
                words.extend(args);
                break;
            }
            _ if arg.starts_with('-') => bail!("unknown option {arg}"),
            _ => words.push(arg),
        }
    }
    let emotions: Vec<Emotion> = serde_json::from_str(&catalog)?;
    if emotions.is_empty() || emotions.iter().any(|e| e.examples.is_empty()) {
        bail!("empty catalog or examples");
    }
    let start = Instant::now();
    let model = StaticModel::from_pretrained(&model, None, Some(true), None)?;
    let prototypes: Vec<_> = emotions.iter().map(|e| model.encode(&e.examples)).collect();
    if scores {
        eprintln!(
            "model + catalog load: {:.1} ms",
            start.elapsed().as_secs_f64() * 1e3
        );
    }
    let select = |text: &str| -> Result<()> {
        if text.trim().is_empty() {
            bail!("provide some text");
        }
        let start = Instant::now();
        let q = model.encode_single(text);
        let mut ranked: Vec<_> = emotions
            .iter()
            .zip(&prototypes)
            .map(|(e, examples)| {
                let score = examples
                    .iter()
                    .map(|v| q.iter().zip(v).map(|(a, b)| a * b).sum::<f32>())
                    .fold(f32::NEG_INFINITY, f32::max);
                (&e.token, score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let winner = if q.iter().all(|&x| x == 0.0) {
            "neutral"
        } else {
            ranked[0].0.as_str()
        };
        let elapsed = start.elapsed();
        println!("{winner}");
        if scores {
            for (label, score) in ranked.iter().take(5) {
                eprintln!("{label:12} {score:.4}");
            }
            eprintln!("selection: {:.3} ms", elapsed.as_secs_f64() * 1e3);
        }
        Ok(())
    };
    if !words.is_empty() {
        return select(&words.join(" "));
    }
    for line in io::stdin().lock().lines() {
        let line = line?;
        if !line.trim().is_empty() {
            select(&line)?;
        }
    }
    Ok(())
}
