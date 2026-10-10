use criterion_platform::auxv::{Auxv, Cache, Failure, ReadError};
use std::io::Read;

#[test]
fn page_size_and_valid_zero_are_distinct_from_a_missing_unsigned_key() {
    // Literal ARM32 LE entries: AT_PAGESZ=4096, AT_FLAGS=0, high unsigned type, AT_NULL.
    let bytes = [
        6, 0, 0, 0, 0, 16, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 120, 86, 52, 18, 0, 0,
        0, 0, 0, 0, 0, 0,
    ];
    let mut reader = bytes.as_slice();
    let auxv = Auxv::read(|out| reader.read(out).map_err(|_| ReadError::Unavailable)).unwrap();
    assert_eq!(auxv.lookup(6), Some(4096));
    assert_eq!(auxv.lookup(8), Some(0));
    assert_eq!(auxv.lookup(u32::MAX), Some(0x12345678));
    assert_eq!(auxv.lookup(27), None);
    assert_eq!(auxv.lookup(0), None);
}

#[test]
fn missing_or_failed_auxv_sets_linux_enoent_and_does_not_retry_a_failed_cache() {
    let cache = Cache::new();
    let mut errno = 47;
    let result = cache.getauxval(u32::MAX, &mut errno, || Err(Failure::Unavailable));
    assert_eq!((result, errno), (0, 2));
    let result = cache.getauxval(6, &mut errno, || panic!("failed cache must be immutable"));
    assert_eq!((result, errno), (0, 2));
    let cache = Cache::new();
    let result = cache.getauxval(0, &mut errno, || {
        let mut bytes = [0u8; 8].as_slice();
        Auxv::read(|out| bytes.read(out).map_err(|_| ReadError::Unavailable))
    });
    assert_eq!((result, errno), (0, 2));
}

#[test]
fn immutable_cache_loads_once_across_threads_and_preserves_present_zero_errno() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let cache = Cache::new();
    let loads = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let mut errno = 47;
                let value = cache.getauxval(8, &mut errno, || {
                    loads.fetch_add(1, Ordering::SeqCst);
                    let mut bytes = [8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0].as_slice();
                    Auxv::read(|out| bytes.read(out).map_err(|_| ReadError::Unavailable))
                });
                assert_eq!((value, errno), (0, 47));
            });
        }
    });
    assert_eq!(loads.load(Ordering::SeqCst), 1);
}

#[test]
fn partial_reads_and_interruptions_preserve_whole_little_endian_pairs() {
    let mut bytes = [6, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0].as_slice();
    let mut calls = 0;
    let vector = Auxv::read(|out| {
        calls += 1;
        if calls % 3 == 0 {
            return Err(ReadError::Interrupted);
        }
        bytes
            .read(&mut out[..1])
            .map_err(|_| ReadError::Unavailable)
    })
    .unwrap();
    assert_eq!(vector.lookup(6), Some(4096));
    assert!(calls < 30);
    let mut big_endian = [0, 0, 0, 6, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0].as_slice();
    let vector =
        Auxv::read(|out| big_endian.read(out).map_err(|_| ReadError::Unavailable)).unwrap();
    assert_eq!(vector.lookup(6), None);
    assert_eq!(vector.lookup(0x06000000), Some(0x00100000));
}

#[test]
fn malformed_proc_data_cannot_publish_a_partial_or_duplicate_vector() {
    for bytes in [
        &[][..],
        &[6, 0, 0, 0, 0, 16, 0][..],
        &[6, 0, 0, 0, 0, 16, 0, 0][..],
        &[0, 0, 0, 0, 1, 0, 0, 0][..],
        &[0, 0, 0, 0, 0, 0, 0, 0, 0][..],
        &[
            6, 0, 0, 0, 0, 16, 0, 0, 6, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ][..],
    ] {
        let mut reader = bytes;
        assert!(matches!(
            Auxv::read(|out| reader.read(out).map_err(|_| ReadError::Unavailable)),
            Err(Failure::Malformed)
        ));
    }
    assert!(matches!(
        Auxv::read(|out| Ok(out.len() + 1)),
        Err(Failure::Malformed)
    ));
    assert!(matches!(
        Auxv::read(|_| Err(ReadError::Unavailable)),
        Err(Failure::Unavailable)
    ));
}

#[test]
fn oversized_vectors_and_unending_interruptions_have_finite_work() {
    let mut bytes = Vec::new();
    for kind in 1u32..=64 {
        bytes.extend(kind.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
    }
    bytes.extend([0; 8]);
    let mut reader = bytes.as_slice();
    assert!(matches!(
        Auxv::read(|out| reader.read(out).map_err(|_| ReadError::Unavailable)),
        Err(Failure::Limit)
    ));
    let mut calls = 0;
    assert!(matches!(
        Auxv::read(|_| {
            calls += 1;
            Err(ReadError::Interrupted)
        }),
        Err(Failure::Limit)
    ));
    assert_eq!(calls, 1024);
}

#[test]
fn unsigned_c_result_preserves_all_bits_and_success_errno() {
    let cache = Cache::new();
    let mut errno = 11;
    let value = cache.getauxval(u32::MAX, &mut errno, || {
        let mut bytes = [
            255, 255, 255, 255, 118, 152, 220, 254, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
        .as_slice();
        Auxv::read(|out| bytes.read(out).map_err(|_| ReadError::Unavailable))
    });
    assert_eq!((value, errno), (0xfedc9876, 11));
}

#[test]
fn zero_terminator_with_sixteen_zero_padding_bytes_publishes_the_cached_vector() {
    // Synthetic entries with the observed zero-valued AT_NULL + 16-zero-byte tail.
    let bytes = [
        6, 0, 0, 0, 0, 16, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 120, 86, 52, 18, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let cache = Cache::new();
    let mut errno = 47;
    let page_size = cache.getauxval(6, &mut errno, || {
        let mut reader = bytes.as_slice();
        Auxv::read(|out| reader.read(out).map_err(|_| ReadError::Unavailable))
    });
    assert_eq!((page_size, errno), (4096, 47));
    assert_eq!(
        cache.getauxval(8, &mut errno, || panic!(
            "admitted cache must load only once"
        )),
        0
    );
    assert_eq!(errno, 47);
    assert_eq!(
        cache.getauxval(u32::MAX, &mut errno, || panic!(
            "admitted cache must load only once"
        )),
        0x12345678
    );
}

#[test]
fn exact_zero_padding_survives_partial_and_interrupted_reads() {
    for padding in [0, 8, 16] {
        let mut bytes = vec![0; 16 + padding];
        bytes[..8].copy_from_slice(&[6, 0, 0, 0, 0, 16, 0, 0]);
        let mut reader = bytes.as_slice();
        let mut calls = 0;
        let vector = Auxv::read(|out| {
            calls += 1;
            if calls % 3 == 0 {
                return Err(ReadError::Interrupted);
            }
            reader
                .read(&mut out[..1])
                .map_err(|_| ReadError::Unavailable)
        })
        .unwrap();
        assert_eq!(vector.lookup(6), Some(4096));
        assert_eq!(vector.lookup(0), None);
    }
}

#[test]
fn partial_nonzero_and_excess_terminator_padding_remain_malformed() {
    for padding in (1..=32)
        .chain([512])
        .filter(|length| !matches!(length, 8 | 16))
    {
        let bytes = vec![0; 8 + padding];
        let mut reader = bytes.as_slice();
        assert!(
            matches!(
                Auxv::read(|out| reader.read(out).map_err(|_| ReadError::Unavailable)),
                Err(Failure::Malformed)
            ),
            "zero padding length {padding}"
        );
    }
    for padding in [8, 16] {
        for nonzero in 0..padding {
            let mut bytes = vec![0; 8 + padding];
            bytes[8 + nonzero] = 1;
            let mut reader = bytes.as_slice();
            assert!(
                matches!(
                    Auxv::read(|out| reader.read(out).map_err(|_| ReadError::Unavailable)),
                    Err(Failure::Malformed)
                ),
                "nonzero byte {nonzero} in {padding}-byte padding"
            );
        }
    }
}

#[test]
fn termination_keeps_read_errors_and_unending_input_bounded() {
    let mut bytes = [0; 8].as_slice();
    assert!(matches!(
        Auxv::read(|out| {
            if bytes.is_empty() {
                Err(ReadError::Unavailable)
            } else {
                bytes.read(out).map_err(|_| ReadError::Unavailable)
            }
        }),
        Err(Failure::Unavailable)
    ));
    let mut bytes = [0; 8].as_slice();
    assert!(matches!(
        Auxv::read(|out| {
            if bytes.is_empty() {
                Ok(out.len() + 1)
            } else {
                bytes.read(out).map_err(|_| ReadError::Unavailable)
            }
        }),
        Err(Failure::Malformed)
    ));
    let mut bytes = [0; 8].as_slice();
    let mut calls = 0;
    assert!(matches!(
        Auxv::read(|out| {
            calls += 1;
            if bytes.is_empty() {
                Err(ReadError::Interrupted)
            } else {
                bytes.read(out).map_err(|_| ReadError::Unavailable)
            }
        }),
        Err(Failure::Limit)
    ));
    assert_eq!(calls, 1024);
    let mut consumed = 0;
    assert!(matches!(
        Auxv::read(|out| {
            out.fill(0);
            consumed += out.len();
            Ok(out.len())
        }),
        Err(Failure::Malformed)
    ));
    assert_eq!(consumed, 25); // AT_NULL plus the bounded 17-byte padding sentinel.
}
