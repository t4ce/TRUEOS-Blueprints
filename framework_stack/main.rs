use core::convert::Infallible;

use hyper::body::Bytes;
use hyper::{Method, Request, Response, StatusCode, Uri};
use tower::{ServiceBuilder, ServiceExt, service_fn};
use trueos_blueprint::{bp_error, bp_info, runtime, time};

fn main() {
    bp_info!("framework_stack: start");

    bp_info!("framework_stack: stage runtime.current_thread_net.build");
    let runtime = match runtime::current_thread_net().build() {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("framework_stack: runtime build failed: {}", err);
            return;
        }
    };
    bp_info!("framework_stack: success runtime.current_thread_net.build");

    runtime.block_on(async {
        match run_probe().await {
            Ok(()) => bp_info!("framework_stack: done"),
            Err(stage) => bp_error!("framework_stack: failed stage={}", stage),
        }
    });
}

async fn run_probe() -> Result<(), &'static str> {
    probe_hyper_http_shapes()?;
    probe_tower_service_stack().await?;
    probe_tokio_time_surface().await?;
    Ok(())
}

fn probe_hyper_http_shapes() -> Result<(), &'static str> {
    bp_info!("framework_stack: stage hyper.http1.builders");
    let mut client_builder = hyper::client::conn::http1::Builder::new();
    client_builder.http09_responses(false);

    let mut server_builder = hyper::server::conn::http1::Builder::new();
    server_builder.keep_alive(true).half_close(false);
    bp_info!("framework_stack: success hyper.http1.builders");

    bp_info!("framework_stack: stage hyper.request.response.bytes");
    let request = Request::builder()
        .method(Method::POST)
        .uri(Uri::from_static("/trueos/framework"))
        .header("x-trueos-probe", "hyper")
        .body(Bytes::from_static(b"tokio-hyper-tower"))
        .map_err(|_| "hyper.request.build")?;
    if request.method() != Method::POST || request.body().len() != 18 {
        return Err("hyper.request.shape");
    }

    let response = Response::builder()
        .status(StatusCode::ACCEPTED)
        .header("x-trueos-framework", "known-good")
        .body(Bytes::from_static(b"accepted"))
        .map_err(|_| "hyper.response.build")?;
    if response.status() != StatusCode::ACCEPTED || response.body().len() != 8 {
        return Err("hyper.response.shape");
    }
    bp_info!("framework_stack: success hyper.request.response.bytes");

    Ok(())
}

async fn probe_tower_service_stack() -> Result<(), &'static str> {
    bp_info!("framework_stack: stage tower.service_fn.oneshot");
    let service = service_fn(|request: Request<Bytes>| async move {
        let mut response = Response::new(Bytes::from_static(b"tower-ok"));
        if request.uri().path() == "/trueos/framework" {
            *response.status_mut() = StatusCode::OK;
        } else {
            *response.status_mut() = StatusCode::NOT_FOUND;
        }
        Ok::<_, Infallible>(response)
    });

    let request = Request::builder()
        .method(Method::GET)
        .uri(Uri::from_static("/trueos/framework"))
        .body(Bytes::new())
        .map_err(|_| "tower.request.build")?;
    let response = service
        .oneshot(request)
        .await
        .map_err(|_| "tower.service_fn.oneshot")?;
    if response.status() != StatusCode::OK || response.body() != &Bytes::from_static(b"tower-ok") {
        return Err("tower.response.value");
    }
    bp_info!("framework_stack: success tower.service_fn.oneshot");

    bp_info!("framework_stack: stage tower.service_builder.layer");
    let layered = ServiceBuilder::new().map_response(|mut response: Response<Bytes>| {
        response
            .headers_mut()
            .insert("x-trueos-layer", "tower".parse().unwrap());
        response
    });
    let service = layered.service(service_fn(|_request: Request<Bytes>| async move {
        Ok::<_, Infallible>(Response::new(Bytes::from_static(b"layered")))
    }));
    let request = Request::builder()
        .method(Method::GET)
        .uri(Uri::from_static("/layered"))
        .body(Bytes::new())
        .map_err(|_| "tower.layered.request")?;
    let response = service
        .oneshot(request)
        .await
        .map_err(|_| "tower.layered.oneshot")?;
    if response.headers().get("x-trueos-layer").is_none() || response.body().len() != 7 {
        return Err("tower.layered.response");
    }
    bp_info!("framework_stack: success tower.service_builder.layer");

    Ok(())
}

async fn probe_tokio_time_surface() -> Result<(), &'static str> {
    bp_info!("framework_stack: stage tokio.time.timeout");
    let value = time::timeout(time::Duration::from_millis(25), async {
        time::sleep(time::Duration::from_millis(1)).await;
        0xF00Du32
    })
    .await
    .map_err(|_| "tokio.time.timeout.elapsed")?;
    if value != 0xF00D {
        return Err("tokio.time.timeout.value");
    }
    bp_info!("framework_stack: success tokio.time.timeout");
    Ok(())
}
