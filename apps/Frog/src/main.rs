// trueos-blueprint: features=["tokio-net-probe"]

mod weather;

use anyhow::Result;

fn main() -> Result<()> {
    let requests = requests()?;
    let runtime = runtime()?;
    let result = requests.into_iter().try_for_each(|coordinates| {
        let snapshot = runtime.block_on_weather(coordinates)?;
        print_snapshot(&snapshot);
        Ok(())
    });
    if let Err(error) = result.as_ref() {
        eprintln!("Frog weather error: {error:#}");
    }

    runtime.shutdown_background();
    keep_report_open();

    result
}

fn keep_report_open() {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    loop {
        // Completion retires the Matrix slot. Keep the report available until
        // the user closes that slot; sleeping parks the guest without spinning.
        trueos::platform::sleep_ms(1000);
    }
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
            "now: {}C, feels {}C, {}, humidity {}%, wind {} km/h",
            current.temp_c, current.feels_c, current.summary, current.humidity, current.wind_kmh
        );
    }
    for day in &snapshot.days {
        println!(
            "{}: {} — day {}C, feels {}C, range {}..{}C, night {}C, rain {}%, humidity {}%, wind {} km/h {}, UV {}",
            day.weekday,
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
    fn block_on_weather(&self, coordinates: (f64, f64)) -> Result<weather::WeatherSnapshot>;
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
impl RuntimeBlockOn for trueos::runtime::Runtime {
    fn block_on_weather(&self, coordinates: (f64, f64)) -> Result<weather::WeatherSnapshot> {
        self.block_on(weather::load_weather_snapshot(coordinates))
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
impl RuntimeBlockOn for tokio::runtime::Runtime {
    fn block_on_weather(&self, coordinates: (f64, f64)) -> Result<weather::WeatherSnapshot> {
        self.block_on(weather::load_weather_snapshot(coordinates))
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

fn parse_coordinates(input: &str) -> Result<(f64, f64)> {
    let values: Vec<_> = input.split_whitespace().collect();
    anyhow::ensure!(values.len() == 2, "expected longitude latitude");
    let lon: f64 = values[0].parse()?;
    let lat: f64 = values[1].parse()?;
    anyhow::ensure!(
        lon.is_finite()
            && lat.is_finite()
            && (-180.0..=180.0).contains(&lon)
            && (-90.0..=90.0).contains(&lat),
        "invalid longitude/latitude"
    );
    Ok((lon, lat))
}
fn parse_start_script(script: &str) -> Result<Vec<(f64, f64)>> {
    script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let values = line
                .strip_prefix("weather ")
                .ok_or_else(|| anyhow::anyhow!("unknown Frog command: {line}"))?;
            parse_coordinates(values)
        })
        .collect()
}

fn requests() -> Result<Vec<(f64, f64)>> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    match trueos::async_fs::block_on(trueos::async_fs::read_file(b"vFile:launch")) {
        Ok(bytes) => return parse_start_script(&String::from_utf8(bytes)?),
        Err(trueos::async_fs::ERR_NOT_FOUND) => {}
        Err(error) => anyhow::bail!("read Frog start script: {error}"),
    }
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--vmx-minishell")
        .collect();
    if args.is_empty() {
        Ok(vec![(9.456766, 51.832427)])
    } else {
        Ok(vec![parse_coordinates(&args.join(" "))?])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_drains_weather_commands_in_order() {
        assert_eq!(
            parse_start_script("weather 13.8278 51.4713\n\nweather -74 40\n").unwrap(),
            vec![(13.8278, 51.4713), (-74.0, 40.0)]
        );
        assert!(parse_start_script("weather 0 91").is_err());
        assert!(parse_start_script("other 0 0").is_err());
    }
    #[test]
    fn coordinates_are_longitude_first_and_validated() {
        assert_eq!(
            parse_coordinates("13.8278 51.4713\n").unwrap(),
            (13.8278, 51.4713)
        );
        for input in ["NaN 0", "0 91", "181 0", "1", "1 2 3"] {
            assert!(parse_coordinates(input).is_err());
        }
    }
}
