//! A szál processzoridejének mérése (a falióra helyett: a párhuzamosan futó játékok nem torzítják).

/// A hívó szál eddig elhasznált processzoridő-nanomásodperce.
pub fn thread_cpu_ns() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: a `ts` érvényes, kiírható struktúra; a CLOCK_THREAD_CPUTIME_ID minden Linux / macOS rendszeren elérhető.
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    if ok != 0 { 0 } else { ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64 }
}
