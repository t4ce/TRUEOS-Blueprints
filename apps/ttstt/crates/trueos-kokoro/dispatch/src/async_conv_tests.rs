//! Runtime tests use private, explicitly owned pools; the global pool is untouched.
use super::*;
use core::future::Future;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use trueos_ttstt_cpu::prepare_qconv1d;

static RUNTIME_TEST: Mutex<()> = Mutex::new(());

fn cpus(count: usize) -> Option<Vec<usize>> {
    let available = core_affinity::get_core_ids()?;
    if available.len() < count {
        std::eprintln!(
            "async convolution test needs {count} allowed CPUs; available={}",
            available.len()
        );
        return None;
    }
    Some(
        available
            .into_iter()
            .take(count)
            .map(|cpu| cpu.id)
            .collect(),
    )
}

fn drive<F: Future>(future: F) -> F::Output {
    smol::block_on(smol::future::race(future, async {
        smol::Timer::after(Duration::from_secs(20)).await;
        panic!("async convolution runtime made no completion within 20 seconds")
    }))
}

fn fixture(g: Geometry, has_bias: bool) -> (Operation, Vec<f32>) {
    let input: Arc<[f32]> = (0..g.batch * g.input_channels * g.input_width)
        .map(|i| ((i * 37 % 97) as f32 - 53.0) * 0.027)
        .collect::<Vec<_>>()
        .into();
    let reduction = g.input_channels * g.kernel_width;
    let packed: Arc<[i8]> = (0..g.output_channels * reduction)
        .map(|i| (i as u8).wrapping_mul(71).wrapping_add(131) as i8)
        .collect::<Vec<_>>()
        .into();
    let sums: Arc<[i32]> = packed
        .chunks_exact(reduction)
        .map(|row| {
            row.iter()
                .fold(0i32, |sum, &value| sum.wrapping_add(i32::from(value)))
        })
        .collect::<Vec<_>>()
        .into();
    let weights = Arc::new(Weights { packed, sums });
    let float_bias: Vec<_> = (0..g.output_channels)
        .map(|i| i as f32 * 0.013 - 0.12)
        .collect();
    let bias = has_bias.then_some(float_bias.as_slice());
    let mut quantized_bias = vec![0; g.output_channels];
    let prepared = prepare_qconv1d(
        &input,
        g.params(&weights),
        0.03125,
        bias,
        &mut quantized_bias,
    )
    .unwrap();
    let mut expected = vec![0.0; g.batch * g.output_channels * prepared.output_width()];
    Dispatcher::detect()
        .qconv1d_dequantized(
            &input,
            &weights.packed,
            &mut expected,
            &mut vec![0; reduction * 4],
            &mut vec![0; g.output_channels],
            g.params(&weights),
            0.03125,
            bias,
        )
        .unwrap();
    (
        Operation {
            op_index: 0,
            input,
            weights,
            geometry: g,
            prepared,
            bias: has_bias.then(|| Arc::from(quantized_bias)),
        },
        expected,
    )
}

fn geometry(width: usize, channels: usize) -> Geometry {
    Geometry {
        batch: 1,
        input_channels: 64,
        input_width: width,
        output_channels: channels,
        kernel_width: 11,
        stride: 1,
        dilation: 1,
        pad_left: 5,
        pad_right: 5,
        weight_zero: -73,
    }
}

fn assert_bits(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert_eq!(actual.to_bits(), expected.to_bits(), "sample {index}");
    }
}

#[test]
fn two_and_three_smol_workers_match_complete_serial_kernels() {
    let _guard = RUNTIME_TEST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for count in [2, 3] {
        let Some(cpus) = cpus(count) else { continue };
        let pool = Pool::new(&cpus).unwrap();
        let cases = [
            (
                Geometry {
                    batch: 2,
                    input_channels: 35,
                    input_width: 131,
                    output_channels: 7,
                    kernel_width: 7,
                    stride: 2,
                    dilation: 3,
                    pad_left: 9,
                    pad_right: 9,
                    weight_zero: 59,
                },
                true,
            ),
            (
                Geometry {
                    input_channels: 32,
                    input_width: 137,
                    output_channels: 11,
                    dilation: 5,
                    pad_left: 25,
                    pad_right: 25,
                    ..geometry(137, 11)
                },
                false,
            ),
            (
                Geometry {
                    input_channels: 1,
                    input_width: 1,
                    output_channels: 1,
                    kernel_width: 1,
                    pad_left: 0,
                    pad_right: 0,
                    ..geometry(1, 1)
                },
                true,
            ),
        ];
        let mut tiles = 0usize;
        for (g, bias) in cases {
            let (operation, expected) = fixture(g, bias);
            tiles += count.min(g.output_channels) * operation.prepared.output_width().div_ceil(64);
            let actual = drive(pool.evaluate(operation)).unwrap();
            assert_bits(&actual, &expected);
        }
        assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 3);
        assert_eq!(pool.counters.tiles.load(Ordering::Relaxed), tiles as u64);
    }
}

#[test]
fn main_runtime_timer_and_task_progress_while_inference_is_pending() {
    let _guard = RUNTIME_TEST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(cpus) = cpus(2) else { return };
    let pool = Pool::new(&cpus).unwrap();
    let (operation, expected) = fixture(geometry(4097, 64), true);
    let done = AtomicBool::new(false);
    let ticks = AtomicUsize::new(0);
    let turns = AtomicUsize::new(0);
    let ((actual, ()), ()) = drive(smol::future::zip(
        smol::future::zip(
            async {
                let result = pool.evaluate(operation).await;
                done.store(true, Ordering::Release);
                result
            },
            async {
                while !done.load(Ordering::Acquire) {
                    smol::Timer::after(Duration::from_millis(1)).await;
                    if !done.load(Ordering::Acquire)
                        && pool.counters.tiles.load(Ordering::Relaxed) != 0
                    {
                        ticks.fetch_add(1, Ordering::Relaxed);
                    }
                }
            },
        ),
        async {
            while !done.load(Ordering::Acquire) {
                turns.fetch_add(1, Ordering::Relaxed);
                smol::future::yield_now().await;
            }
        },
    ));
    assert_bits(&actual.unwrap(), &expected);
    assert!(
        ticks.load(Ordering::Relaxed) > 0,
        "timer stalled during actual worker execution"
    );
    assert!(
        turns.load(Ordering::Relaxed) > 1,
        "independent async task did not run"
    );
}

#[test]
fn dropping_inflight_evaluation_releases_snapshots_and_pool_remains_usable() {
    let _guard = RUNTIME_TEST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(cpus) = cpus(2) else { return };
    let pool = Pool::new(&cpus).unwrap();
    let (operation, _) = fixture(geometry(4097, 64), true);
    let snapshot = Arc::downgrade(&operation.input);
    drive(async {
        let mut pending = std::boxed::Box::pin(pool.evaluate(operation));
        assert!(smol::future::poll_once(pending.as_mut()).await.is_none());
        while pool.counters.tiles.load(Ordering::Relaxed) == 0 {
            smol::Timer::after(Duration::from_millis(1)).await;
        }
        drop(pending);
        while snapshot.upgrade().is_some() {
            smol::Timer::after(Duration::from_millis(1)).await;
        }
        assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 0);
        let (next, expected) = fixture(geometry(67, 7), false);
        assert_bits(&pool.evaluate(next).await.unwrap(), &expected);
    });
    assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 1);
}

#[test]
fn concurrent_operations_complete_through_bounded_queue_pressure() {
    let _guard = RUNTIME_TEST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for count in [2, 3] {
        let Some(cpus) = cpus(count) else { continue };
        let pool = Pool::new(&cpus).unwrap();
        assert_eq!(pool.jobs.capacity(), Some(count));
        let (first, first_expected) = fixture(geometry(259, 32), true);
        let (second, second_expected) = fixture(geometry(133, 17), false);
        let (third, third_expected) = fixture(geometry(197, 23), true);
        let (fourth, fourth_expected) = fixture(geometry(71, 11), false);
        // Four operations concurrently submit 8/12 shards through a queue of
        // only 2/3 jobs. Bounded sends must park and resume without deadlock.
        let (a, (b, (c, d))) = drive(smol::future::zip(
            pool.evaluate(first),
            smol::future::zip(
                pool.evaluate(second),
                smol::future::zip(pool.evaluate(third), pool.evaluate(fourth)),
            ),
        ));
        assert_bits(&a.unwrap(), &first_expected);
        assert_bits(&b.unwrap(), &second_expected);
        assert_bits(&c.unwrap(), &third_expected);
        assert_bits(&d.unwrap(), &fourth_expected);
        assert_eq!(pool.counters.operations.load(Ordering::Relaxed), 4);
        assert_eq!(pool.jobs.len(), 0);
    }
}

#[test]
fn cancelled_state_wait_is_terminal_and_does_not_report_ready() {
    let _guard = RUNTIME_TEST
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(cpus) = cpus(2) else { return };
    let pool = Pool::new(&cpus).unwrap();
    let (operation, _) = fixture(geometry(4097, 64), false);
    let snapshot = Arc::downgrade(&operation.input);
    let mut state = State {
        pool: Some(pool.clone()),
        pending: None,
    };
    state.stage(123, operation);
    drive(async {
        let mut waiting = std::boxed::Box::pin(state.wait_ready());
        assert!(smol::future::poll_once(waiting.as_mut()).await.is_none());
        while pool.counters.tiles.load(Ordering::Relaxed) == 0 {
            smol::Timer::after(Duration::from_millis(1)).await;
        }
        drop(waiting);
        assert_eq!(
            state.wait_ready().await,
            Err("compute-operation-wait-cancelled")
        );
        assert!(state.pending.as_ref().unwrap().output.is_none());
        while snapshot.upgrade().is_some() {
            smol::Timer::after(Duration::from_millis(1)).await;
        }
        let (next, expected) = fixture(geometry(65, 5), true);
        assert_bits(&pool.evaluate(next).await.unwrap(), &expected);
    });
}
