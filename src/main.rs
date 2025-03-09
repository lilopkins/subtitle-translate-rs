#![deny(unsafe_code)]
#![deny(clippy::pedantic)]

#[allow(unused)]
mod api_types;

use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use api_types::{Query, Translation, TranslationResult};
use aspasia::{Moment, SubRipSubtitle, Subtitle, TimedSubtitleFile, subrip::SubRipEvent};
use clap::Parser;
use clap_verbosity_flag::Verbosity;
use futures::future::TryJoinAll;
use reqwest::Client;
use tokio::sync::Mutex;

#[derive(Parser)]
struct Args {
    /// The URL of the LibreTranslate instance's translation API
    #[arg(short = 'L', long, default_value = "http://localhost:5000/translate")]
    libretranslate_instance: String,

    /// The API key for the LibreTranslate instance, if it is needed
    #[arg(short = 'A', long)]
    libretranslate_apikey: Option<String>,

    /// Set the size of the chunk used for parallel processing
    #[arg(short = 'C', long, default_value_t = 5)]
    chunk_size: usize,

    /// The two letter code for the source language.
    #[arg(short = 'f', long, default_value = "auto")]
    language_from: String,

    /// The source subtitle file
    #[arg(index = 1)]
    source_file: PathBuf,

    /// The two letter code for the target language.
    #[arg(index = 2)]
    language_to: String,

    /// The destination subtitle file
    #[arg(index = 3)]
    destination_file: PathBuf,

    #[command(flatten)]
    verbose: Verbosity,
}

#[derive(Clone, Debug)]
struct GenericSubtitle {
    text: String,
    start: Moment,
    end: Moment,
    coordinates: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_max_level(args.verbose)
        .init();

    // Step 1: Read source subs
    tracing::info!("Reading source subtitles…");
    let subs =
        TimedSubtitleFile::new(args.source_file).context("Failed to read source subtitles")?;
    tracing::debug!("Read subtitles file");
    let subtitles = Arc::new(Mutex::new(timed_subtitle_file_events_to_generic(subs)));

    // Step 2: Translate line by line, asynchronously in batches
    tracing::info!("Translating…");
    let source = args.language_from.to_ascii_lowercase();
    let target = args.language_to.to_ascii_lowercase();
    let client = Client::new();
    {
        let subtitles = subtitles.clone();
        let mut subs = subtitles.lock().await;
        for (chunk_idx, chunk) in subs.chunks_mut(args.chunk_size).enumerate() {
            let handles = chunk.iter().cloned().enumerate().map(|(idx, item)| {
                let inst = args.libretranslate_instance.clone();
                let key = args.libretranslate_apikey.clone();
                let source = source.clone();
                let target = target.clone();
                let client = client.clone();
                let input = item.text.clone();
                let span = tracing::debug_span!("translation", chunk_idx = chunk_idx, idx = idx, input = input);
                tokio::spawn(async move {
                    let _ = span.enter();
                    if input.is_empty() {
                        return Ok(Translation {
                            translated_text: String::new(),
                            alternatives: None,
                            detected_language: None,
                        });
                    }

                    let body = Query {
                        q: input,
                        source: source.clone(),
                        target: target.clone(),
                        api_key: key.clone(),
                        ..Default::default()
                    };
                    tracing::debug!("Sending: {}", serde_json::to_string(&body).unwrap());
                    let res = client.post(inst).json(&body).send().await;
                    match res {
                        Ok(r) => {
                            tracing::trace!("HTTP Response: {r:?}");
                            let r = r.json::<TranslationResult>().await?;
                            tracing::debug!("Response: {r:?}");
                            match r {
                                TranslationResult::Err(e) => Err(anyhow::anyhow!(e.error)),
                                TranslationResult::Ok(r) => Ok(r),
                            }
                        }
                        Err(e) => Err(anyhow::Error::from(e)),
                    }
                })
            });

            let results = handles.collect::<TryJoinAll<_>>().await?;
            for (idx, result) in results.into_iter().enumerate() {
                let translation = result.context("Failed to translate line")?;
                chunk[idx].text = translation.translated_text;
            }
        }
    }

    // Step 3: Write final file
    tracing::info!("Writing translated subtitles…");
    let real_target = {
        let mut p = args.destination_file;
        p.set_extension("srt");
        p
    };
    tracing::debug!("Real destination is {real_target:?}");

    tracing::debug!("Converting subtitles back into SRT events");
    let mut events = vec![];
    for (idx, subtitle) in subtitles.lock().await.iter().enumerate() {
        events.push(SubRipEvent {
            line_number: idx + 1,
            text: subtitle.text.clone(),
            start: subtitle.start,
            end: subtitle.end,
            coordinates: subtitle.coordinates.clone(),
        });
    }

    let mut srt = SubRipSubtitle::from_events(events);
    srt.renumber();
    srt.export(real_target)
        .context("Failed to write destination subtitle file")?;

    Ok(())
}

fn timed_subtitle_file_events_to_generic(subs: TimedSubtitleFile) -> Vec<GenericSubtitle> {
    let mut subtitles = vec![];
    match subs {
        TimedSubtitleFile::Ass(ass) => subtitles.append(
            &mut ass
                .events()
                .iter()
                .map(|ev| GenericSubtitle {
                    text: ev.text.clone(),
                    start: ev.start,
                    end: ev.end,
                    coordinates: None,
                })
                .collect(),
        ),
        TimedSubtitleFile::MicroDvd(dvd) => subtitles.append(
            &mut dvd
                .events()
                .iter()
                .map(|ev| GenericSubtitle {
                    text: ev.text.clone(),
                    start: ev.start,
                    end: ev.end,
                    coordinates: None,
                })
                .collect(),
        ),
        TimedSubtitleFile::Ssa(ssa) => subtitles.append(
            &mut ssa
                .events()
                .iter()
                .map(|ev| GenericSubtitle {
                    text: ev.text.clone(),
                    start: ev.start,
                    end: ev.end,
                    coordinates: None,
                })
                .collect(),
        ),
        TimedSubtitleFile::SubRip(srt) => subtitles.append(
            &mut srt
                .events()
                .iter()
                .map(|ev| GenericSubtitle {
                    text: ev.text.clone(),
                    start: ev.start,
                    end: ev.end,
                    coordinates: ev.coordinates.clone(),
                })
                .collect(),
        ),
        TimedSubtitleFile::WebVtt(vtt) => subtitles.append(
            &mut vtt
                .events()
                .iter()
                .map(|ev| GenericSubtitle {
                    text: ev.text.clone(),
                    start: ev.start,
                    end: ev.end,
                    coordinates: None,
                })
                .collect(),
        ),
    }
    subtitles
}
