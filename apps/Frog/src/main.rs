// trueos-blueprint: features=["tokio-net-probe"]

mod weather;

use anyhow::Result;

fn main() -> Result<()> {
    let runtime = runtime()?;
    let result = runtime.block_on_weather().map(|snapshot| {
        print_snapshot(&snapshot);
    });
    if let Err(error) = result.as_ref() {
        eprintln!("Frog weather error: {error:#}");
    }

    runtime.shutdown_background();
    shutdown_blueprint(if result.is_ok() {
        "Frog printed live weather"
    } else {
        "Frog weather request failed"
    });

    result
}

fn shutdown_blueprint(reason: &str) {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        trueos::vshell::leave_terminal_handoff();
        let _ = trueos::vshell::shutdown_current_blueprint(reason);
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let _ = reason;
}

fn print_snapshot(snapshot: &weather::WeatherSnapshot) {
    println!(
        "Frog weather: {}, {} ({:.4}, {:.4})",
        snapshot.location.name,
        snapshot.location.country,
        snapshot.location.lat,
        snapshot.location.lon
    );
    println!("source: {}", snapshot.source);
    if let Some(current) = snapshot.current.as_ref() {
        println!(
            "now: {} {}C, feels {}C, {}, humidity {}%, wind {} km/h",
            current.icon.glyph(),
            current.temp_c,
            current.feels_c,
            current.summary,
            current.humidity,
            current.wind_kmh
        );
    }
    for day in &snapshot.days {
        println!(
            "{}: {} {} — day {}C, feels {}C, range {}..{}C, night {}C, rain {}%, humidity {}%, wind {} km/h {}, UV {}",
            day.weekday,
            day.icon.glyph(),
            day.summary,
            day.temp_day_c,
            day.feels_day_c,
            day.temp_min_c,
            day.temp_max_c,
            day.temp_night_c,
            day.rain_percent,
            day.humidity,
            day.wind_kmh,
            day.wind_dir,
            day.uvi
        );
    }
    if !snapshot.note.is_empty() {
        println!("note: {}", snapshot.note);
    }
}

trait RuntimeBlockOn {
    fn block_on_weather(&self) -> Result<weather::WeatherSnapshot>;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl RuntimeBlockOn for trueos::runtime::Runtime {
    fn block_on_weather(&self) -> Result<weather::WeatherSnapshot> {
        self.block_on(weather::load_weather_snapshot())
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl RuntimeBlockOn for tokio::runtime::Runtime {
    fn block_on_weather(&self) -> Result<weather::WeatherSnapshot> {
        self.block_on(weather::load_weather_snapshot())
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn runtime() -> Result<trueos::runtime::Runtime> {
    Ok(trueos::runtime::current_thread_net().build()?)
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}
