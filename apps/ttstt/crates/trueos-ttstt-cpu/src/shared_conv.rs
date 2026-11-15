//! Immutable time-major activation preparation shared by channel workers.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct SharedQConv1dRequirements {
    pub cache_bytes: usize,
    pub column_sums: usize,
    pub patch_sums: usize,
}

impl QConv1dParams<'_> {
    pub fn shared_cache_requirements(self) -> Result<SharedQConv1dRequirements, Error> {
        let width = self.output_width()?;
        let columns = self
            .input_width
            .checked_add(self.pad_left)
            .and_then(|v| v.checked_add(self.pad_right))
            .ok_or(Error::ShapeOverflow)?;
        Ok(SharedQConv1dRequirements {
            cache_bytes: self
                .batch
                .checked_mul(columns)
                .and_then(|v| v.checked_mul(self.input_channels))
                .ok_or(Error::ShapeOverflow)?,
            column_sums: columns,
            patch_sums: self.batch.checked_mul(width).ok_or(Error::ShapeOverflow)?,
        })
    }
}

impl Dispatcher {
    /// Quantize each input column once, with exact global parameters and padding.
    /// Cache layout is [batch, padded_input_time, input_channel]. Patch sums are
    /// computed from column sums once, independently of the output-channel split.
    /// Input must be the unchanged finite tensor passed to `prepare_qconv1d`.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare_qconv1d_shared_cache_with_lane(
        self,
        input: &[f32],
        cache: &mut [u8],
        column_sums: &mut [i32],
        patch_sums: &mut [i32],
        params: QConv1dParams<'_>,
        prepared: PreparedQConv1d,
        lane: Lane,
    ) -> Result<(), Error> {
        if prepared.geometry != qconv1d_geometry(params) {
            return Err(Error::PreparedConvolutionMismatch);
        }
        let needs = params.shared_cache_requirements()?;
        let input_len = params
            .batch
            .checked_mul(params.input_channels)
            .and_then(|v| v.checked_mul(params.input_width))
            .ok_or(Error::ShapeOverflow)?;
        if input.len() < input_len {
            return Err(Error::LhsTooSmall);
        }
        if cache.len() < needs.cache_bytes
            || column_sums.len() < needs.column_sums
            || patch_sums.len() < needs.patch_sums
        {
            return Err(Error::ScratchTooSmall);
        }
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let params = QConv1dParams {
            input_zero_point: prepared.quantization.zero_point,
            ..params
        };
        let columns = needs.column_sums;
        for batch in 0..params.batch {
            let cache = &mut cache[batch * columns * params.input_channels
                ..(batch + 1) * columns * params.input_channels];
            #[cfg(target_arch = "x86_64")]
            if lane != Lane::Scalar {
                // AVX2 gathers use the same per-sample rounding as the scalar lane.
                gather_qconv1d_cache_f32(
                    input,
                    cache,
                    0..columns,
                    params,
                    prepared.quantization,
                    batch,
                    0,
                )?;
            } else {
                prepare_scalar_columns(input, cache, params, prepared, batch, columns);
            }
            #[cfg(not(target_arch = "x86_64"))]
            prepare_scalar_columns(input, cache, params, prepared, batch, columns);
            for (column, values) in cache.chunks_exact(params.input_channels).enumerate() {
                column_sums[column] = sum_u8(values);
            }
            for time in 0..prepared.output_width {
                let mut sum = 0i32;
                for tap in 0..params.kernel_width {
                    sum =
                        sum.wrapping_add(column_sums[time * params.stride + tap * params.dilation]);
                }
                patch_sums[batch * prepared.output_width + time] = sum;
            }
        }
        Ok(())
    }

    /// Compute a compact [batch, channel_count, time_count] output using an
    /// immutable prepared cache. No activation quantization, gather or patch-sum
    /// calculation is repeated by the channel workers. Integer dot products and
    /// the float epilogue are the original operations.
    #[allow(clippy::too_many_arguments)]
    pub fn qconv1d_shared_cache_window_with_lane(
        self,
        cache: &[u8],
        patch_sums: &[i32],
        weights: &[i8],
        output: &mut [f32],
        params: QConv1dParams<'_>,
        prepared: PreparedQConv1d,
        bias: Option<&[i32]>,
        channel_start: usize,
        channel_count: usize,
        time_start: usize,
        time_count: usize,
        lane: Lane,
    ) -> Result<(), Error> {
        if prepared.geometry != qconv1d_geometry(params) || prepared.has_bias != bias.is_some() {
            return Err(Error::PreparedConvolutionMismatch);
        }
        let channel_end = channel_start
            .checked_add(channel_count)
            .ok_or(Error::InvalidOutputChannelRange)?;
        if channel_count == 0 || channel_end > params.output_channels {
            return Err(Error::InvalidOutputChannelRange);
        }
        let time_end = time_start
            .checked_add(time_count)
            .ok_or(Error::InvalidOutputTimeRange)?;
        if time_count == 0 || time_end > prepared.output_width {
            return Err(Error::InvalidOutputTimeRange);
        }
        params.dequantized_scratch_requirements(prepared.has_bias)?;
        let needs = params.shared_cache_requirements()?;
        if cache.len() < needs.cache_bytes || patch_sums.len() < needs.patch_sums {
            return Err(Error::ScratchTooSmall);
        }
        let reduction = params
            .input_channels
            .checked_mul(params.kernel_width)
            .ok_or(Error::ShapeOverflow)?;
        let weight_elements = params
            .output_channels
            .checked_mul(reduction)
            .ok_or(Error::ShapeOverflow)?;
        if weights.len() < weight_elements {
            return Err(Error::RhsTooSmall);
        }
        if bias.is_some_and(|v| v.len() < params.output_channels) {
            return Err(Error::BiasTooSmall);
        }
        let output_len = params
            .batch
            .checked_mul(channel_count)
            .and_then(|v| v.checked_mul(time_count))
            .ok_or(Error::ShapeOverflow)?;
        if output.len() < output_len {
            return Err(Error::OutputTooSmall);
        }
        if !self.supports(lane) {
            return Err(Error::UnsupportedLane);
        }
        let params = QConv1dParams {
            input_zero_point: prepared.quantization.zero_point,
            ..params
        };
        for batch in 0..params.batch {
            let begin =
                (batch * needs.column_sums + time_start * params.stride) * params.input_channels;
            let cache = &cache[begin..(batch + 1) * needs.column_sums * params.input_channels];
            let sums = &patch_sums[batch * prepared.output_width + time_start..];
            let mut time = 0;
            #[cfg(target_arch = "x86_64")]
            if lane == Lane::AvxVnni {
                while time + 4 <= time_count {
                    for channel in channel_start..channel_end {
                        let row = &weights[channel * reduction..(channel + 1) * reduction];
                        // SAFETY: lane support, full cache geometry and bounds were validated above.
                        let raw =
                            unsafe { raw_qconv1d_dot4_cached_avx_vnni(cache, row, params, time) };
                        let weight_sum = params
                            .weight_row_sums
                            .map_or_else(|| sum_i8(row), |v| v[channel]);
                        for position in 0..4 {
                            let value = apply_zero_points(
                                raw[position],
                                sums[time + position],
                                weight_sum,
                                reduction,
                                params.input_zero_point,
                                params.weight_zero_points.get(channel),
                            )
                            .wrapping_add(bias.map_or(0, |v| v[channel]));
                            output[(batch * channel_count + channel - channel_start)
                                * time_count
                                + time
                                + position] = value as f32 * prepared.combined_scale;
                        }
                    }
                    time += 4;
                }
            }
            while time < time_count {
                for channel in channel_start..channel_end {
                    let row = &weights[channel * reduction..(channel + 1) * reduction];
                    let raw = qconv1d_dot_cached(cache, row, params, time, lane);
                    let weight_sum = params
                        .weight_row_sums
                        .map_or_else(|| sum_i8(row), |v| v[channel]);
                    let value = apply_zero_points(
                        raw,
                        sums[time],
                        weight_sum,
                        reduction,
                        params.input_zero_point,
                        params.weight_zero_points.get(channel),
                    )
                    .wrapping_add(bias.map_or(0, |v| v[channel]));
                    output[(batch * channel_count + channel - channel_start) * time_count + time] =
                        value as f32 * prepared.combined_scale;
                }
                time += 1;
            }
        }
        Ok(())
    }
}

fn prepare_scalar_columns(
    input: &[f32],
    cache: &mut [u8],
    params: QConv1dParams<'_>,
    prepared: PreparedQConv1d,
    batch: usize,
    columns: usize,
) {
    for column in 0..columns {
        let values =
            &mut cache[column * params.input_channels..(column + 1) * params.input_channels];
        let Some(time) = column
            .checked_sub(params.pad_left)
            .filter(|&t| t < params.input_width)
        else {
            values.fill(prepared.quantization.zero_point);
            continue;
        };
        for (channel, value) in values.iter_mut().enumerate() {
            *value = quant::quantize_value_u8(
                input[(batch * params.input_channels + channel) * params.input_width + time],
                prepared.quantization,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cache_matches_serial_bits_for_lanes_channels_windows_and_geometry() {
        let dispatcher = Dispatcher::detect();
        for (batch, ci, width, co, kernel, stride, dilation, pad) in [
            (2, 35, 37, 7, 7, 1, 1, 3),
            (1, 32, 29, 5, 11, 1, 5, 25),
            (2, 3, 13, 5, 3, 2, 3, 3),
            (2, 1, 1, 1, 1, 1, 1, 0),
        ] {
            let input: Vec<_> = (0..batch * ci * width)
                .map(|i| ((i * 67 % 131) as f32 - 73.0) * 0.019)
                .collect();
            let weights: Vec<_> = (0..co * ci * kernel)
                .map(|i| (i as u8).wrapping_mul(31).wrapping_add(71) as i8)
                .collect();
            let sums: Vec<_> = weights.chunks_exact(ci * kernel).map(sum_i8).collect();
            let zeros: Vec<_> = (0..co)
                .map(|i| (i as i8).wrapping_mul(53).wrapping_sub(99))
                .collect();
            let bias: Vec<_> = (0..co).map(|i| i as f32 * 0.04 - 0.11).collect();
            for has_bias in [false, true] {
                for have_sums in [false, true] {
                    for per_output in [false, true] {
                        let params = QConv1dParams {
                            batch,
                            input_channels: ci,
                            input_width: width,
                            output_channels: co,
                            kernel_width: kernel,
                            stride,
                            dilation,
                            pad_left: pad,
                            pad_right: pad,
                            input_zero_point: 19,
                            weight_zero_points: if per_output {
                                RhsZeroPoints::PerOutput(&zeros)
                            } else {
                                RhsZeroPoints::Scalar(-73)
                            },
                            weight_row_sums: have_sums.then_some(sums.as_slice()),
                        };
                        let bias = has_bias.then_some(bias.as_slice());
                        let mut qb = vec![0; co];
                        let prepared =
                            prepare_qconv1d(&input, params, 0.0625, bias, &mut qb).unwrap();
                        let needs = params.shared_cache_requirements().unwrap();
                        for lane in [Lane::Scalar, Lane::Avx2, Lane::AvxVnni]
                            .into_iter()
                            .filter(|&lane| dispatcher.supports(lane))
                        {
                            let mut cache = vec![19; needs.cache_bytes + 3];
                            let mut ps = vec![47; needs.patch_sums + 3];
                            dispatcher
                                .prepare_qconv1d_shared_cache_with_lane(
                                    &input,
                                    &mut cache,
                                    &mut vec![0; needs.column_sums],
                                    &mut ps,
                                    params,
                                    prepared,
                                    lane,
                                )
                                .unwrap();
                            assert_eq!(&cache[needs.cache_bytes..], &[19; 3]);
                            assert_eq!(&ps[needs.patch_sums..], &[47; 3]);
                            let mut expected = vec![0.; batch * co * prepared.output_width()];
                            dispatcher
                                .qconv1d_dequantized_with_lane(
                                    &input,
                                    &weights,
                                    &mut expected,
                                    &mut vec![0; ci * kernel * 4],
                                    &mut vec![0; co],
                                    params,
                                    0.0625,
                                    bias,
                                    lane,
                                )
                                .unwrap();
                            for quantum in [1usize, 4, 7, 64] {
                                let mut actual = vec![f32::NAN; expected.len()];
                                for channel in (0..co).step_by(3) {
                                    let count = 3.min(co - channel);
                                    for time in (0..prepared.output_width()).step_by(quantum) {
                                        let times = quantum.min(prepared.output_width() - time);
                                        let n = batch * count * times;
                                        let mut shard = vec![123.5; n + 3];
                                        dispatcher
                                            .qconv1d_shared_cache_window_with_lane(
                                                &cache,
                                                &ps,
                                                &weights,
                                                &mut shard,
                                                params,
                                                prepared,
                                                has_bias.then_some(qb.as_slice()),
                                                channel,
                                                count,
                                                time,
                                                times,
                                                lane,
                                            )
                                            .unwrap();
                                        assert_eq!(&shard[n..], &[123.5; 3]);
                                        for b in 0..batch {
                                            for c in 0..count {
                                                actual[(b * co + channel + c)
                                                    * prepared.output_width()
                                                    + time
                                                    ..(b * co + channel + c)
                                                        * prepared.output_width()
                                                        + time
                                                        + times]
                                                    .copy_from_slice(
                                                        &shard[(b * count + c) * times
                                                            ..(b * count + c + 1) * times],
                                                    );
                                            }
                                        }
                                    }
                                }
                                assert_eq!(
                                    actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                                    "lane={lane:?} ci={ci} kernel={kernel} dilation={dilation} bias={has_bias} sums={have_sums} per_output={per_output} quantum={quantum}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_shared_buffers_and_windows_do_not_modify_destinations() {
        let dispatcher = Dispatcher::detect();
        let params = QConv1dParams {
            batch: 1,
            input_channels: 1,
            input_width: 3,
            output_channels: 1,
            kernel_width: 3,
            stride: 1,
            dilation: 1,
            pad_left: 1,
            pad_right: 1,
            input_zero_point: 0,
            weight_zero_points: RhsZeroPoints::Scalar(0),
            weight_row_sums: None,
        };
        let prepared = prepare_qconv1d(&[-1., 0., 1.], params, 0.5, None, &mut []).unwrap();
        let mut cache = [77; 5];
        let mut sums = [33; 3];
        assert_eq!(
            dispatcher.prepare_qconv1d_shared_cache_with_lane(
                &[-1., 0., 1.],
                &mut cache[..4],
                &mut [0; 5],
                &mut sums,
                params,
                prepared,
                Lane::Scalar
            ),
            Err(Error::ScratchTooSmall)
        );
        assert_eq!(cache, [77; 5]);
        assert_eq!(sums, [33; 3]);
        let mut out = [123.5; 3];
        assert_eq!(
            dispatcher.qconv1d_shared_cache_window_with_lane(
                &cache,
                &sums,
                &[1; 3],
                &mut out,
                params,
                prepared,
                None,
                0,
                1,
                2,
                2,
                Lane::Scalar
            ),
            Err(Error::InvalidOutputTimeRange)
        );
        assert_eq!(out, [123.5; 3]);
        assert_eq!(
            dispatcher.qconv1d_shared_cache_window_with_lane(
                &cache,
                &sums,
                &[1; 3],
                &mut out,
                params,
                prepared,
                None,
                1,
                1,
                0,
                3,
                Lane::Scalar
            ),
            Err(Error::InvalidOutputChannelRange)
        );
        assert_eq!(out, [123.5; 3]);
    }
}
