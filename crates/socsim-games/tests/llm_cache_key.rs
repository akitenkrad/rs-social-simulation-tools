use socsim_games::llm::{BackendIdentity, CacheKeyV1};

fn key() -> CacheKeyV1 {
    CacheKeyV1 {
        version: 1,
        backend: BackendIdentity {
            model: "model-a".to_owned(),
            endpoint: "memory://a".to_owned(),
        },
        system: Some("system".to_owned()),
        prompt: "prompt".to_owned(),
        temperature_bits: 0.2_f32.to_bits(),
        seed: 7,
        omit_seed: false,
        max_tokens: Some(128),
        think: Some(true),
        allow_blank: false,
    }
}

#[test]
fn canonical_json_uses_the_declared_field_order() {
    let key = key();

    assert_eq!(
        key.canonical_json(),
        concat!(
            r#"{"version":1,"backend":{"model":"model-a","endpoint":"memory://a"},"#,
            r#""system":"system","prompt":"prompt","temperature_bits":1045220557,"#,
            r#""seed":7,"omit_seed":false,"max_tokens":128,"think":true,"allow_blank":false}"#,
        )
    );
}

#[test]
fn digest_is_fnv1a_64_of_the_complete_canonical_json() {
    let key = key();
    let expected = key
        .canonical_json()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });

    assert_eq!(key.digest(), expected);
}

#[test]
fn every_cache_input_changes_the_canonical_key() {
    let base = key();
    let variants = [
        CacheKeyV1 {
            version: 2,
            ..base.clone()
        },
        CacheKeyV1 {
            backend: BackendIdentity {
                model: "model-b".to_owned(),
                ..base.backend.clone()
            },
            ..base.clone()
        },
        CacheKeyV1 {
            backend: BackendIdentity {
                endpoint: "memory://b".to_owned(),
                ..base.backend.clone()
            },
            ..base.clone()
        },
        CacheKeyV1 {
            system: None,
            ..base.clone()
        },
        CacheKeyV1 {
            prompt: "different".to_owned(),
            ..base.clone()
        },
        CacheKeyV1 {
            temperature_bits: 0.8_f32.to_bits(),
            ..base.clone()
        },
        CacheKeyV1 {
            seed: 8,
            ..base.clone()
        },
        CacheKeyV1 {
            omit_seed: true,
            ..base.clone()
        },
        CacheKeyV1 {
            max_tokens: None,
            ..base.clone()
        },
        CacheKeyV1 {
            think: Some(false),
            ..base.clone()
        },
        CacheKeyV1 {
            allow_blank: true,
            ..base.clone()
        },
    ];

    for variant in variants {
        assert_ne!(variant.canonical_json(), base.canonical_json());
        assert_ne!(variant.digest(), base.digest());
    }
}

#[test]
fn temperature_uses_its_exact_f32_bits() {
    let low = key();
    let high = CacheKeyV1 {
        temperature_bits: 0.8_f32.to_bits(),
        ..low.clone()
    };
    let positive_zero = CacheKeyV1 {
        temperature_bits: 0.0_f32.to_bits(),
        ..low.clone()
    };
    let negative_zero = CacheKeyV1 {
        temperature_bits: (-0.0_f32).to_bits(),
        ..low
    };

    assert_ne!(high.canonical_json(), negative_zero.canonical_json());
    assert_ne!(positive_zero.digest(), negative_zero.digest());
}
