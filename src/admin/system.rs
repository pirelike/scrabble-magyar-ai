//! Admin panel: rendszer — naplópuffer, háttérfolyamatok, konfiguráció, adatbázis, mentés, takarítás.

use crate::app::App;
use std::sync::Arc;

/// A konzolra írt üzenetek gyűjtése az admin panel naplónézetéhez (a Python `install_log_capture` megfelelője).
pub fn install_log_capture(_app: &Arc<App>) {}
