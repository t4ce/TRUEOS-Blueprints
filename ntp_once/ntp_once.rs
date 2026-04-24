#![no_std]
#![no_main]

use trueos::{bp_error, bp_info, vclock};

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let unix = vclock::ntp_current_unix_seconds();
    let date = vclock::ntp_kernel_date_day_month_year();

    match (unix, date) {
        (Some(unix), Some(date)) => {
            bp_info!("ntp_once bp: unix={} date={}", unix, date);
        }
        (Some(unix), None) => {
            bp_info!("ntp_once bp: unix={} date=<unavailable>", unix);
        }
        (None, Some(date)) => {
            bp_info!("ntp_once bp: unix=<unavailable> date={}", date);
        }
        (None, None) => {
            bp_error!("ntp_once bp: kernel ntp unavailable");
        }
    }
}
