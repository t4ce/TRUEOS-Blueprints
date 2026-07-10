#![cfg_attr(not(test), no_std)]

pub mod complex;
pub use complex::Complex;

pub mod matrix;
pub use matrix::{Matrix, Vector};

pub mod calculator_base;

#[inline]
pub fn sin_f32(x: f32) -> f32 {
    libm::sinf(x)
}
#[inline]
pub fn cos_f32(x: f32) -> f32 {
    libm::cosf(x)
}
#[inline]
pub fn acos_f32(x: f32) -> f32 {
    libm::acosf(x)
}
#[inline]
pub fn asin_f32(x: f32) -> f32 {
    libm::asinf(x)
}
#[inline]
pub fn log2_f32(x: f32) -> f32 {
    libm::log2f(x)
}
#[inline]
pub fn log_f32(x: f32) -> f32 {
    libm::logf(x)
}
#[inline]
pub fn log10_f32(x: f32) -> f32 {
    libm::log10f(x)
}
#[inline]
pub fn exp_f32(x: f32) -> f32 {
    libm::expf(x)
}
#[inline]
pub fn pow_f32(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}
#[inline]
pub fn tanh_f32(x: f32) -> f32 {
    libm::tanhf(x)
}
#[inline]
pub fn hypot_f32(x: f32, y: f32) -> f32 {
    libm::hypotf(x, y)
}
#[inline]
pub fn sin_f64(x: f64) -> f64 {
    libm::sin(x)
}
#[inline]
pub fn cos_f64(x: f64) -> f64 {
    libm::cos(x)
}
#[inline]
pub fn log2_f64(x: f64) -> f64 {
    libm::log2(x)
}
#[inline]
pub fn log_f64(x: f64) -> f64 {
    libm::log(x)
}
#[inline]
pub fn log10_f64(x: f64) -> f64 {
    libm::log10(x)
}
#[inline]
pub fn exp_f64(x: f64) -> f64 {
    libm::exp(x)
}
#[inline]
pub fn pow_f64(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}
#[inline]
pub fn tanh_f64(x: f64) -> f64 {
    libm::tanh(x)
}
#[inline]
pub fn hypot_f64(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}
#[inline]
pub fn atan2_f64(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
