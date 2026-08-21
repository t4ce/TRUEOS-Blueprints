use std::time::Duration;

use anyhow::{Context, Result, anyhow};

const WEATHER_API_KEY: &str = "9715912a7d8748d65bc3985b4a4274a0";
const FROG_LATITUDE: f64 = 51.832427;
const FROG_LONGITUDE: f64 = 9.456766;
const FALLBACK_CITY: &str = "Holzminden";
const FALLBACK_COUNTRY: &str = "DE";
const FETCH_TIMEOUT_MS: u64 = 45_000;
const DAILY_ROW_COUNT: usize = 8;

#[derive(Clone, Debug)]
pub struct WeatherSnapshot {
    pub location: Location,
    pub source: String,
    pub current: Option<CurrentWeather>,
    pub days: Vec<ForecastDay>,
    pub note: String,
}

#[derive(Clone, Debug)]
pub struct Location {
    pub name: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Clone, Debug)]
pub struct CurrentWeather {
    pub summary: String,
    pub temp_c: i32,
    pub feels_c: i32,
    pub humidity: i32,
    pub wind_kmh: i32,
}

#[derive(Clone, Debug)]
pub struct ForecastDay {
    pub weekday: &'static str,
    pub summary: String,
    pub temp_day_c: i32,
    pub temp_min_c: i32,
    pub temp_max_c: i32,
    pub temp_night_c: i32,
    pub feels_day_c: i32,
    pub rain_percent: i32,
    pub humidity: i32,
    pub wind_kmh: i32,
    pub wind_dir: &'static str,
    pub uvi: i32,
}

pub async fn load_weather_snapshot() -> Result<WeatherSnapshot> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(FETCH_TIMEOUT_MS))
        .tls_danger_accept_invalid_certs(true)
        .build()
        .context("build weather http client")?;

    let mut note = String::new();
    let location = match load_location(&client).await {
        Ok(location) => location,
        Err(err) => {
            note = format!("reverse geo failed: {err}; using saved location");
            fallback_location()
        }
    };

    let raw_weather = fetch_text(&client, forecast_url(&location).as_str())
        .await
        .context("fetch live OpenWeather forecast")?;

    let response = trueos_weather::oc3::decode_onecall_raw_safe(raw_weather.as_str())
        .map_err(|_| anyhow!("decode OpenWeather onecall response"))?;

    Ok(build_snapshot(
        location,
        response,
        String::from("live OpenWeather 3.0 onecall metric/de"),
        note,
    ))
}

fn fallback_location() -> Location {
    Location {
        name: String::from(FALLBACK_CITY),
        country: String::from(FALLBACK_COUNTRY),
        lat: FROG_LATITUDE,
        lon: FROG_LONGITUDE,
    }
}

async fn load_location(client: &reqwest::Client) -> Result<Location> {
    let raw = fetch_text(
        client,
        trueos_weather::oc3::openweather_geo_url(FROG_LATITUDE, FROG_LONGITUDE, WEATHER_API_KEY)
            .as_str(),
    )
    .await?;
    parse_reverse_geo(raw.as_str()).context("empty reverse geo response")
}

fn forecast_url(location: &Location) -> String {
    format!(
        "{}?lat={}&lon={}&exclude=minutely,hourly,alerts&units=metric&lang=de&appid={}",
        trueos_weather::config::ONECALL_URL,
        location.lat,
        location.lon,
        WEATHER_API_KEY
    )
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("request {url}"))?;
    let status = response.status();
    let body = response.bytes().await.context("read response body")?;
    if !status.is_success() {
        return Err(anyhow!("http {}", status.as_u16()));
    }
    String::from_utf8(body.to_vec()).context("response was not utf8")
}

fn parse_reverse_geo(raw: &str) -> Option<Location> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let first = root.as_array()?.first()?;
    Some(Location {
        name: first.get("name")?.as_str()?.to_string(),
        country: first.get("country")?.as_str()?.to_string(),
        lat: first
            .get("lat")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(FROG_LATITUDE),
        lon: first
            .get("lon")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(FROG_LONGITUDE),
    })
}

fn build_snapshot(
    location: Location,
    response: trueos_weather::OpenWeatherResponse,
    source: String,
    note: String,
) -> WeatherSnapshot {
    let current = response.current.as_ref().map(|current| {
        let weather = current.weather.first();
        CurrentWeather {
            summary: weather
                .map(|w| title_case(w.description.as_str()))
                .unwrap_or_else(|| String::from("Weather")),
            temp_c: rounded(current.temp),
            feels_c: rounded(current.feels_like),
            humidity: current.humidity,
            wind_kmh: ms_to_kmh(current.wind_speed),
        }
    });

    let days = response
        .daily
        .as_ref()
        .map(|daily| {
            daily
                .iter()
                .take(DAILY_ROW_COUNT)
                .map(|day| {
                    let weather = day.weather.first();
                    ForecastDay {
                        weekday: weekday_abbrev(day.dt),
                        summary: if day.summary.is_empty() {
                            weather
                                .map(|w| title_case(w.description.as_str()))
                                .unwrap_or_else(|| String::from("Weather"))
                        } else {
                            day.summary.clone()
                        },
                        temp_day_c: rounded(day.temp.day),
                        temp_min_c: rounded(day.temp.min),
                        temp_max_c: rounded(day.temp.max),
                        temp_night_c: rounded(day.temp.night),
                        feels_day_c: rounded(day.feels_like.day),
                        rain_percent: rounded(day.pop * 100.0),
                        humidity: day.humidity,
                        wind_kmh: ms_to_kmh(day.wind_speed),
                        wind_dir: cardinal(day.wind_deg),
                        uvi: rounded(day.uvi),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    WeatherSnapshot {
        location,
        source,
        current,
        days,
        note,
    }
}

fn weekday_abbrev(unix: u64) -> &'static str {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    DAYS[((unix / 86_400) % 7) as usize]
}

fn cardinal(deg: i32) -> &'static str {
    const DIRS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    let idx = (((deg as f64 + 11.25) / 22.5).floor() as usize) % DIRS.len();
    DIRS[idx]
}

fn title_case(input: &str) -> String {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_uppercase().chain(chars).collect()
}

fn rounded(value: f64) -> i32 {
    value.round() as i32
}

fn ms_to_kmh(value: f64) -> i32 {
    rounded(value * 3.6)
}
