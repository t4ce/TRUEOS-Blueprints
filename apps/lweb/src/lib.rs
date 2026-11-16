//! Server-rendered local applications. Register additional apps in APPS and router().
use axum::{
    Router,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use frog::weather::{self, WeatherSnapshot};
use serde::Deserialize;
use std::{
    fmt::Write,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const APPS: &[(&str, &str, &str)] = &[(
    "/weather",
    "Weather",
    "Current conditions and an eight-day outlook from Frog.",
)];
const DEFAULT_LON: f64 = 9.456766;
const DEFAULT_LAT: f64 = 51.832427;

type Cache = Arc<Mutex<Option<Cached>>>;
struct Cached {
    point: (f64, f64),
    fetched: Instant,
    result: Result<WeatherSnapshot, String>,
}

pub fn router() -> Router {
    Router::new()
        .route("/", get(home))
        .route("/weather", get(forecast))
        .route(
            "/assets/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../assets/style.css"),
                )
            }),
        )
        .route("/assets/weather/{name}", get(icon_asset))
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Html(page(
                    "Page not found",
                    "<h1>Page not found</h1><p><a href=\"/\">Open local apps</a></p>",
                )),
            )
        })
        .with_state(Cache::default())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn page(title: &str, body: &str) -> String {
    let mut nav = String::from("<a href=\"/\">All apps</a>");
    for (path, name, _) in APPS {
        let _ = write!(nav, "<a href=\"{path}\">{name}</a>");
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{} · lweb</title><link rel=\"stylesheet\" href=\"/assets/style.css\"></head><body><header><p class=\"brand\">lweb</p><p>Little local websites</p><nav>{nav}</nav></header><main>{body}</main><footer>Local apps · Weather powered by Frog / OpenWeather · <a href=\"/assets/weather/LICENSE\">Weather icon license</a></footer></body></html>",
        escape(title)
    )
}

async fn home() -> Html<String> {
    let mut body = String::from(
        "<h1>A small home for local apps.</h1><p class=\"muted\">Choose an app to open.</p>",
    );
    for (path, name, description) in APPS {
        let _ = write!(
            body,
            "<section class=\"panel\"><h2><a href=\"{path}\">{name}</a></h2><p>{description}</p><a href=\"/weather?demo=true\">Preview with sample weather</a></section>"
        );
    }
    Html(page("Local apps", &body))
}

#[derive(Deserialize)]
struct Selection {
    #[serde(default = "default_lon")]
    lon: f64,
    #[serde(default = "default_lat")]
    lat: f64,
    #[serde(default)]
    demo: bool,
}
fn default_lon() -> f64 {
    DEFAULT_LON
}
fn default_lat() -> f64 {
    DEFAULT_LAT
}

async fn forecast(State(cache): State<Cache>, Query(selection): Query<Selection>) -> Response {
    let point = (selection.lon, selection.lat);
    if !point.0.is_finite()
        || !point.1.is_finite()
        || !(-180.0..=180.0).contains(&point.0)
        || !(-90.0..=90.0).contains(&point.1)
    {
        return (StatusCode::BAD_REQUEST, Html(page("Invalid location", "<h1>Invalid location</h1><p>Longitude must be between -180 and 180; latitude between -90 and 90.</p>"))).into_response();
    }
    let result = if selection.demo {
        weather::demo_snapshot().map_err(|_| "The sample forecast could not be loaded.".into())
    } else {
        // Serialize upstream requests and reuse results. Failed requests have a short
        // backoff, so repeated navigation cannot hammer the weather service.
        let mut slot = cache.lock().await;
        let fresh = slot.as_ref().is_some_and(|c| {
            c.point == point
                && c.fetched.elapsed()
                    < if c.result.is_ok() {
                        Duration::from_secs(300)
                    } else {
                        Duration::from_secs(30)
                    }
        });
        if !fresh {
            let result = weather::load_weather_snapshot(point).await.map_err(|_| {
                // Frog errors can contain upstream URLs; keep credentials out of HTML/logs.
                "Live weather is unavailable. Try again shortly, or open the sample forecast."
                    .to_string()
            });
            *slot = Some(Cached {
                point,
                fetched: Instant::now(),
                result,
            });
        }
        slot.as_ref().unwrap().result.clone()
    };
    match result {
        Ok(snapshot) => Html(page("Weather", &render_weather(&snapshot, selection.demo, point))).into_response(),
        Err(message) => (StatusCode::BAD_GATEWAY, Html(page("Weather unavailable", &format!("<h1>Weather unavailable</h1><p class=\"notice\">{}</p><p><a href=\"/weather?lon={}&amp;lat={}\">Try live weather again</a></p><p><a href=\"/weather?demo=true\">Open sample forecast</a></p>", escape(&message), point.0, point.1)))).into_response(),
    }
}

fn icon_name(code: &str) -> &'static str {
    match code {
        "01d" => "clear-day",
        "01n" => "clear-night",
        "02d" | "03d" => "cloudy-2-day",
        "02n" | "03n" => "cloudy-2-night",
        "09d" | "09n" => "rainy-2",
        "10d" | "10n" => "rainy-1-day",
        "11d" | "11n" => "thunderstorms",
        "13d" | "13n" => "snowy-1",
        "50d" | "50n" => "fog-day",
        _ => "cloudy",
    }
}
fn icon(code: &str) -> String {
    format!(
        "<img class=\"weather-icon\" src=\"/assets/weather/{}.png\" alt=\"\" width=\"64\" height=\"64\">",
        icon_name(code)
    )
}

fn render_weather(snapshot: &WeatherSnapshot, demo: bool, point: (f64, f64)) -> String {
    let mut body = String::from("<h1>Weather</h1>");
    if demo {
        body.push_str(
            "<p class=\"notice\">Sample forecast · Offline fixture, not live conditions.</p>",
        );
    }
    let _ = write!(
        body,
        "<p>{} {} · {:.4}, {:.4}</p><p class=\"muted\">{}</p>",
        escape(&snapshot.location.name),
        escape(&snapshot.location.country),
        snapshot.location.lat,
        snapshot.location.lon,
        escape(&snapshot.local_datetime)
    );
    let _ = write!(
        body,
        "<p><a href=\"/weather?lon={}&amp;lat={}&amp;demo={}\">Reload weather</a> · <a href=\"/\">All apps</a></p>",
        point.0, point.1, demo
    );
    if let Some(now) = &snapshot.current {
        let _ = write!(
            body,
            "<section class=\"panel current\">{}<div><h2>Right now</h2><p class=\"temperature\">{} °C</p><p>{}</p><p>Feels like {} °C · Humidity {}% · Wind {} km/h</p></div></section>",
            icon(&now.icon),
            now.temp_c,
            escape(&now.summary),
            now.feels_c,
            now.humidity,
            now.wind_kmh
        );
    } else {
        body.push_str("<p class=\"notice\">Current conditions are unavailable.</p>");
    }
    body.push_str("<section class=\"panel\"><h2>Daily outlook</h2><table aria-label=\"Daily weather forecast\"><thead><tr><th class=\"day\" scope=\"col\">Day</th><th class=\"conditions\" scope=\"col\">Conditions</th><th class=\"value\" scope=\"col\">Temp</th><th class=\"value\" scope=\"col\">Rain</th><th class=\"value\" scope=\"col\">Wind / UV</th></tr></thead><tbody>");
    for day in &snapshot.days {
        let _ = write!(
            body,
            "<tr><th class=\"day\" scope=\"row\">{}</th><td class=\"conditions\">{}<span>{}</span></td><td class=\"value\">Day {} °C<span class=\"detail\">{} to {} °C</span><span class=\"detail\">Night {} °C</span><span class=\"detail\">Feels {} °C</span></td><td class=\"value\">Rain {}%<span class=\"detail\">Humidity {}%</span></td><td class=\"value\">{} km/h {}<span class=\"detail\">UV {}</span></td></tr>",
            day.weekday,
            icon(&day.icon),
            escape(&day.summary),
            day.temp_day_c,
            day.temp_min_c,
            day.temp_max_c,
            day.temp_night_c,
            day.feels_day_c,
            day.rain_percent,
            day.humidity,
            day.wind_kmh,
            day.wind_dir,
            day.uvi
        );
    }
    body.push_str("</tbody></table></section>");
    if snapshot.days.is_empty() {
        body.push_str("<p class=\"notice\">Daily forecast data is unavailable.</p>");
    }
    if !demo && !snapshot.note.is_empty() {
        body.push_str("<p class=\"notice\">Location name unavailable; showing weather at the selected coordinates.</p>");
    }
    if !demo {
        body.push_str("<p class=\"muted\">Live responses are cached for five minutes. Reload opens a fresh page.</p>");
    }
    body
}

async fn icon_asset(Path(name): Path<String>) -> Response {
    macro_rules! image {
        ($file:literal) => {
            include_bytes!(concat!("../assets/weather/", $file, ".png")).as_slice()
        };
    }
    let bytes = match name.as_str() {
        "clear-day.png" => image!("clear-day"),
        "clear-night.png" => image!("clear-night"),
        "cloudy-2-day.png" => image!("cloudy-2-day"),
        "cloudy-2-night.png" => image!("cloudy-2-night"),
        "cloudy.png" => image!("cloudy"),
        "rainy-1-day.png" => image!("rainy-1-day"),
        "rainy-2.png" => image!("rainy-2"),
        "snowy-1.png" => image!("snowy-1"),
        "thunderstorms.png" => image!("thunderstorms"),
        "fog-day.png" => image!("fog-day"),
        "LICENSE" => {
            return (
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                include_str!("../assets/weather/LICENSE"),
            )
                .into_response();
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn offline_routes_render_real_frog_data_and_local_assets() {
        let app = router();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/weather?demo=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("Offline fixture"));
        assert!(html.contains("<table"));
        assert!(!html.contains("<script"));
        let snapshot = weather::demo_snapshot().unwrap();
        assert!(!snapshot.days.is_empty());
        for day in &snapshot.days {
            assert!(html.contains(&escape(&day.summary)));
        }
        for code in [
            "01d", "01n", "02d", "03n", "04d", "09d", "10n", "11d", "13d", "50n", "unknown",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/assets/weather/{}.png", icon_name(code)))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        }
        for (uri, status) in [
            ("/", StatusCode::OK),
            ("/assets/style.css", StatusCode::OK),
            ("/assets/weather/LICENSE", StatusCode::OK),
            ("/weather?lon=NaN", StatusCode::BAD_REQUEST),
            ("/weather?lat=91", StatusCode::BAD_REQUEST),
            ("/assets/weather/missing.png", StatusCode::NOT_FOUND),
            ("/missing", StatusCode::NOT_FOUND),
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                status,
                "{uri}"
            );
        }
    }

    #[test]
    fn weather_text_is_escaped_and_missing_data_has_a_message() {
        let mut snapshot = weather::demo_snapshot().unwrap();
        snapshot.location.name = "<script>alert('x')</script>".into();
        snapshot.days.clear();
        snapshot.current = None;
        let html = render_weather(&snapshot, true, (0.0, 0.0));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("Daily forecast data is unavailable"));
    }
}
