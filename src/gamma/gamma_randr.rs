use xcb;
use xcb::randr;
use transition;
use colorramp;

use super::GammaMethod;
use super::Result;
use std::error::Error;
use std::fmt;

const RANDR_MAJOR_VERSION: u32 = 1;
const RANDR_MINOR_VERSION: u32 = 3;

/// Wrapper for XCB and RandR errors
pub enum RandrError<T> {
    Generic(xcb::Error<T>),
    Conn(xcb::ConnError),
    UnsupportedVersion(u32, u32)
}

impl<T: 'static> RandrError<T> {
    fn generic(e: xcb::Error<T>) -> Box<Error> {
        Box::new(RandrError::Generic(e)) as Box<Error>
    }
}

impl RandrError<()> {
    fn conn(e: xcb::ConnError) -> Box<Error> {
        Box::new(RandrError::Conn::<()>(e)) as Box<Error>
    }

    fn unsupported_version(major: u32, minor: u32) -> Box<Error> {
        Box::new(RandrError::UnsupportedVersion::<()>(major, minor)) as Box<Error>
    }
}

impl<T> fmt::Display for RandrError<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl<T> fmt::Debug for RandrError<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::RandrError::*;
        match *self {
            Generic(ref e) =>
                write!(f, "randr error: {}", e.error_code()),
            Conn(xcb::ConnError::Connection) =>
                write!(f, "xcb connection errors because of socket, pipe or other stream errors"),
            Conn(ref c) =>
                write!(f, "{:?}", c),
            UnsupportedVersion(major, minor) =>
                write!(f, "Unsupported RandR version ({}.{})", major, minor),
        }
    }
}

impl<T> Error for RandrError<T> {
    fn description(&self) -> &str {
        "RandR error"
    }
}

struct Crtc {
    /// The id of CRTC (gotten from XCB)
    id: u32,

    /// The ramp size.
    ramp_size: u32,

    /// The initial gamma ramp values - used for restore
    saved_ramps: (Vec<u16>, Vec<u16>, Vec<u16>),

    /// A scratchpad for color computation - it saves the cost of
    /// allocating three new arrays whenever set_temperature() is
    /// called.
    scratch: (Vec<u16>, Vec<u16>, Vec<u16>),
}

/// Wrapping struct for RandR state
pub struct RandrState {
    conn: xcb::Connection,
    screen_num: i32,
    window_dummy: u32,
    crtcs: Vec<Crtc>
}

impl RandrState {

    fn init() -> Result<RandrState> {
        let (conn, screen_num) = xcb::Connection::connect(None)
            .map_err(RandrError::conn)?;

        query_version(&conn)?;

        let window_dummy = {
            let setup = conn.get_setup();
            let screen = setup.roots().nth(screen_num as usize).unwrap();
            let window_dummy = conn.generate_id();

            xcb::create_window(&conn, 0, window_dummy, screen.root(), 0, 0, 1,
                               1, 0, 0, 0, &[]);
            conn.flush();
            window_dummy
        };

        Ok(RandrState {
            conn: conn,
            screen_num: screen_num,
            window_dummy: window_dummy,
            crtcs: vec![]
        })
    }

    // Set the temperature for the indicated CRTC
    fn set_crtc_temperatures(&mut self, setting: &transition::ColorSetting) -> Result<()> {
        for crtc in self.crtcs.iter_mut() {

            let (ref mut r, ref mut g, ref mut b) = crtc.scratch;

            let u16_max1 = u16::max_value() as f64 + 1.0;
            let ramp_size = crtc.ramp_size as f64;
            for i in 0 .. r.len() {
                let v = ((i as f64 / ramp_size) * u16_max1) as u16;
                r[i] = v;
                g[i] = v;
                b[i] = v;
            }

            /* Create new gamma ramps */
            colorramp::fill(&mut r[..], &mut g[..], &mut b[..],
                            setting, crtc.ramp_size as usize);

            randr::set_crtc_gamma_checked(&self.conn,
                                          crtc.id,
                                          &r[..],
                                          &g[..],
                                          &b[..])
                .request_check()
                .map_err(RandrError::generic)?;
        }
        Ok(())
    }
}

fn query_version(conn: &xcb::Connection) -> Result<()> {
    let reply = randr::query_version(conn,
                                     RANDR_MAJOR_VERSION,
                                     RANDR_MINOR_VERSION)
        .get_reply()
        .map_err(RandrError::generic)?;
    if reply.major_version() != RANDR_MAJOR_VERSION || reply.minor_version() < RANDR_MINOR_VERSION {
        Err(RandrError::unsupported_version(reply.major_version(),
                                            reply.minor_version()))
    } else {
        Ok(())
    }
}

impl GammaMethod for RandrState {

    //
    // Restore saved gamma ramps
    //
    fn restore(&self) -> Result<()> {
        for crtc in self.crtcs.iter() {
            randr::set_crtc_gamma_checked(&self.conn,
                                          crtc.id,
                                          &crtc.saved_ramps.0[..],
                                          &crtc.saved_ramps.1[..],
                                          &crtc.saved_ramps.2[..])
                .request_check()
                .map_err(RandrError::generic)?;
        }
        Ok(())
    }

    fn set_temperature(&mut self, setting: &transition::ColorSetting) -> Result<()> {
        self.set_crtc_temperatures(setting)
    }

    /// Find initial information on all the CRTCs
    fn start(&mut self) -> Result<()> {
        /* Get list of CRTCs for the screen */
        let screen_resources = randr::get_screen_resources(&self.conn,
                                                           self.window_dummy)
            .get_reply()
            .map_err(RandrError::generic)?;
        self.crtcs = Vec::with_capacity(screen_resources.num_crtcs() as usize);

        /* Save size and gamma ramps of all CRTCs */
        for crtc in screen_resources.crtcs() {
            let gamma = randr::get_crtc_gamma(&self.conn, *crtc)
                .get_reply()
                .map_err(RandrError::generic)?;

            let red = gamma.red().to_vec();
            let green = gamma.green().to_vec();
            let blue = gamma.blue().to_vec();

            self.crtcs.push(Crtc {
                id: *crtc,
                ramp_size: gamma.size() as u32,
                saved_ramps: (red.clone(), green.clone(), blue.clone()),
                scratch: (red, green, blue),
            });
        }
        Ok(())
    }
}

/// The init function
pub fn init() -> Result<Box<GammaMethod>> {
    RandrState::init().map(|r| Box::new(r) as Box<GammaMethod>)
}
