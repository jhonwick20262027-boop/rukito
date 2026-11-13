//! Suite U de subida de archivos (§20).
//!
//! Los casos U-01 a U-06 corren en proceso y sin red. U-07 solo se ejecuta si
//! existe `RDD_TEST_<PROVEEDOR>_URL` contra un emulador; se salta en otro caso.

#![cfg(feature = "upload")]

use std::sync::Arc;

use bytes::Bytes;
use rdd::upload::{
    DEFAULT_MAX_OBJECT_BYTES, MemoryStorage, ObjectKey, ObjectStorage, PutOptions, Secret,
    StoredObject,
};

// U-01: la memoria comparte la semántica de los adaptadores.
#[tokio::test]
async fn u01_memory_storage_put_get_head_delete_y_limite() {
    let storage: Arc<dyn ObjectStorage> = Arc::new(MemoryStorage::new());
    let key = ObjectKey::parse("files/a/uno.txt").unwrap();
    let body = Bytes::from_static(b"contenido");

    let meta = storage
        .put(
            &key,
            body.clone(),
            PutOptions::new()
                .content_type("text/plain")
                .metadata("origen", "prueba"),
        )
        .await
        .unwrap();
    assert_eq!(meta.size, body.len() as u64);
    assert_eq!(meta.content_type.as_deref(), Some("text/plain"));
    assert!(meta.etag.is_some());

    let StoredObject {
        meta: head_meta,
        body: read,
    } = storage.get(&key).await.unwrap().unwrap();
    assert_eq!(read, body);
    assert_eq!(head_meta, meta);
    assert_eq!(storage.head(&key).await.unwrap().unwrap(), meta);

    // Sobrescribir cambia el cuerpo y el tamaño.
    let updated = storage
        .put(&key, Bytes::from_static(b"otro"), PutOptions::new())
        .await
        .unwrap();
    assert_eq!(updated.size, 4);
    assert_eq!(
        storage.get(&key).await.unwrap().unwrap().body,
        Bytes::from_static(b"otro")
    );

    // `delete` es idempotente y luego `get` devuelve `None`.
    storage.delete(&key).await.unwrap();
    storage.delete(&key).await.unwrap();
    assert!(storage.get(&key).await.unwrap().is_none());
    assert!(storage.head(&key).await.unwrap().is_none());

    // El límite se aplica antes de guardar.
    let small = MemoryStorage::with_max_bytes(2);
    let err = small
        .put(&key, Bytes::from_static(b"tres"), PutOptions::new())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("límite"), "{err}");
    assert!(small.get(&key).await.unwrap().is_none());

    // Dos tareas concurrentes no corrompen el mapa.
    let storage = Arc::new(MemoryStorage::new());
    let mut tasks = Vec::new();
    for index in 0..8 {
        let storage = Arc::clone(&storage);
        tasks.push(tokio::spawn(async move {
            let key = ObjectKey::parse(format!("files/{index}.txt")).unwrap();
            storage
                .put(
                    &key,
                    Bytes::from(format!("cuerpo {index}")),
                    PutOptions::new(),
                )
                .await
                .unwrap();
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    for index in 0..8 {
        let key = ObjectKey::parse(format!("files/{index}.txt")).unwrap();
        assert_eq!(
            storage.get(&key).await.unwrap().unwrap().body,
            Bytes::from(format!("cuerpo {index}"))
        );
    }
    assert_eq!(DEFAULT_MAX_OBJECT_BYTES, 64 * 1024 * 1024);
}

// U-02: validación y codificación de claves.
#[test]
fn u02_object_key_valida_y_codifica_por_segmento() {
    for raw in [
        "",
        "/files/a.txt",
        "files/../a.txt",
        "files/./a.txt",
        "files//a.txt",
        "files\\a.txt",
        "files/a\u{7}b.txt",
    ] {
        assert!(ObjectKey::parse(raw).is_err(), "aceptó `{raw}`");
    }
    let larga = "a".repeat(1025);
    assert!(ObjectKey::parse(&larga).is_err());

    let key = ObjectKey::parse("mis archivos/año 1.txt").unwrap();
    assert_eq!(key.encoded_path(), "mis%20archivos/a%C3%B1o%201.txt");
    assert_eq!(key.as_str(), "mis archivos/año 1.txt");
    assert_eq!(
        ObjectKey::parse("a".repeat(1024)).unwrap().as_str().len(),
        1024
    );
}

// U-03: S3 usa path-style con endpoint y virtual-host sin él.
#[cfg(feature = "upload-aws")]
#[test]
fn u03_s3_elige_path_style_y_valida_el_endpoint() {
    use rdd::upload::{Credentials, S3Config, S3Storage};

    let config = S3Config::new("mi-bucket", Credentials::new("AKIA", "secreto"));
    assert!(!config.uses_path_style());
    assert_eq!(config.region_value(), "us-east-1");

    let local = config.clone().endpoint("http://127.0.0.1:9000");
    assert!(local.uses_path_style());
    assert!(S3Storage::new(local).is_ok());

    let forced = config.force_path_style(true);
    assert!(forced.uses_path_style());
    assert!(S3Storage::new(forced).is_ok());
}

// U-04: Azure acepta el endpoint de Azurite y el de la nube.
#[cfg(feature = "upload-azure")]
#[test]
fn u04_azure_acepta_endpoint_de_azurite() {
    use rdd::upload::{AzureBlobConfig, AzureBlobStorage};

    // Clave por defecto del emulador Azurite (documentación pública).
    const AZURITE_KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    let nube = AzureBlobConfig::new("devstoreaccount1", AZURITE_KEY, "mi-contenedor");
    assert!(AzureBlobStorage::new(nube).is_ok());

    let azurite = AzureBlobConfig::new("devstoreaccount1", AZURITE_KEY, "mi-contenedor")
        .endpoint("http://127.0.0.1:10000/devstoreaccount1");
    assert!(AzureBlobStorage::new(azurite).is_ok());
}

// U-05: GCS sirve con el endpoint público y con uno alternativo.
#[cfg(feature = "upload-gcs")]
#[test]
fn u05_gcs_acepta_endpoint_alternativo() {
    use rdd::upload::{GcsConfig, GcsStorage};

    let nube = GcsConfig::new("mi-bucket", "GOOG1EXAMPLEACCESSID", "secreto");
    assert!(GcsStorage::new(nube).is_ok());

    let local = GcsConfig::new("mi-bucket", "GOOG1EXAMPLEACCESSID", "secreto")
        .endpoint("http://127.0.0.1:4443");
    assert!(GcsStorage::new(local).is_ok());
}

// U-06: precedencia constructor > entorno > defecto y validación de endpoint.
#[test]
fn u06_precedencia_defectos_y_secretos() {
    let secret = Secret::new("valor-secreto");
    assert!(!format!("{secret:?}").contains("valor-secreto"));
    assert!(!format!("{secret}").contains("valor-secreto"));

    #[cfg(feature = "upload-aws")]
    {
        use rdd::upload::{Credentials, S3Config, S3Storage};

        let env = |name: &str| match name {
            "AWS_S3_BUCKET" => Some("bucket-env".to_owned()),
            "AWS_ACCESS_KEY_ID" => Some("AKIA-ENV".to_owned()),
            "AWS_SECRET_ACCESS_KEY" => Some("secreto-env".to_owned()),
            "AWS_REGION" => Some("eu-west-1".to_owned()),
            _ => None,
        };
        let config = S3Config::from_lookup(env).unwrap();
        assert_eq!(config.region_value(), "eu-west-1");
        assert_eq!(config.max_bytes(), DEFAULT_MAX_OBJECT_BYTES);
        // El constructor pisa la variable.
        assert_eq!(
            config.clone().region("us-west-2").region_value(),
            "us-west-2"
        );
        assert_eq!(config.max_object_bytes(1024).max_bytes(), 1024);

        // Un secreto ausente falla nombrando la variable.
        let missing = S3Config::from_lookup(|_| None).unwrap_err().to_string();
        assert!(missing.contains("AWS_S3_BUCKET"), "{missing}");

        // `ftp://` se rechaza; `http://` solo se acepta escrito de forma explícita.
        let base = S3Config::new("mi-bucket", Credentials::new("AKIA", "secreto"));
        let err = match S3Storage::new(base.clone().endpoint("ftp://host")) {
            Ok(_) => panic!("aceptó un endpoint ftp"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("https"), "{err}");
        assert!(S3Storage::new(base.endpoint("http://127.0.0.1:9000")).is_ok());
    }

    #[cfg(feature = "upload-azure")]
    {
        use rdd::upload::AzureBlobConfig;

        const AZURITE_KEY: &str = "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";
        let env = |name: &str| match name {
            "AZURE_STORAGE_ACCOUNT" => Some("cuentaenv".to_owned()),
            "AZURE_STORAGE_ACCESS_KEY" => Some(AZURITE_KEY.to_owned()),
            "AZURE_STORAGE_CONTAINER" => Some("contenedor-env".to_owned()),
            _ => None,
        };
        let config = AzureBlobConfig::from_lookup(env).unwrap();
        assert_eq!(config.account(), "cuentaenv");
        assert_eq!(config.max_bytes(), DEFAULT_MAX_OBJECT_BYTES);
        let missing = AzureBlobConfig::from_lookup(|_| None)
            .unwrap_err()
            .to_string();
        assert!(missing.contains("AZURE_STORAGE_ACCOUNT"), "{missing}");
    }

    #[cfg(feature = "upload-gcs")]
    {
        use rdd::upload::GcsConfig;

        let env = |name: &str| match name {
            "GCS_BUCKET" => Some("bucket-env".to_owned()),
            "GCS_HMAC_ACCESS_ID" => Some("id-env".to_owned()),
            "GCS_HMAC_SECRET" => Some("secreto-env".to_owned()),
            _ => None,
        };
        let config = GcsConfig::from_lookup(env).unwrap();
        assert_eq!(config.bucket(), "bucket-env");
        assert_eq!(config.max_bytes(), DEFAULT_MAX_OBJECT_BYTES);
        let missing = GcsConfig::from_lookup(|_| None).unwrap_err().to_string();
        assert!(missing.contains("GCS_BUCKET"), "{missing}");
    }
}

// U-07: round-trip opcional contra un emulador.
async fn round_trip(storage: &dyn ObjectStorage, clave: ObjectKey) {
    let body = Bytes::from_static(b"hola emulador");
    let meta = storage
        .put(
            &clave,
            body.clone(),
            PutOptions::new().content_type("text/plain"),
        )
        .await
        .unwrap();
    assert_eq!(meta.size, body.len() as u64);

    let head = storage.head(&clave).await.unwrap().unwrap();
    assert_eq!(head.size, body.len() as u64);
    assert_eq!(head.content_type.as_deref(), Some("text/plain"));

    let object = storage.get(&clave).await.unwrap().unwrap();
    assert_eq!(object.body, body);

    storage.delete(&clave).await.unwrap();
    assert!(storage.get(&clave).await.unwrap().is_none());
}

fn test_key() -> ObjectKey {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    ObjectKey::parse(format!("rdd-test/{}-{nanos}.txt", std::process::id())).unwrap()
}

#[cfg(feature = "upload-aws")]
#[tokio::test]
async fn u07_round_trip_s3_si_hay_emulador() {
    use rdd::upload::{Credentials, S3Config, S3Storage};

    let Ok(url) = std::env::var("RDD_TEST_S3_URL") else {
        return;
    };
    let bucket = std::env::var("RDD_TEST_S3_BUCKET").unwrap_or_else(|_| "rdd-test".to_owned());
    let access =
        std::env::var("RDD_TEST_AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_owned());
    let secret =
        std::env::var("RDD_TEST_AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_owned());
    let storage = S3Storage::new(
        S3Config::new(bucket, Credentials::new(access, secret))
            .endpoint(url)
            .force_path_style(true),
    )
    .unwrap();
    round_trip(&storage, test_key()).await;
}

#[cfg(feature = "upload-azure")]
#[tokio::test]
async fn u07_round_trip_azure_si_hay_emulador() {
    use rdd::upload::{AzureBlobConfig, AzureBlobStorage};

    let Ok(url) = std::env::var("RDD_TEST_AZURE_URL") else {
        return;
    };
    let account =
        std::env::var("RDD_TEST_AZURE_ACCOUNT").unwrap_or_else(|_| "devstoreaccount1".to_owned());
    let key = std::env::var("RDD_TEST_AZURE_KEY").unwrap_or_else(|_| {
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw=="
            .to_owned()
    });
    let container =
        std::env::var("RDD_TEST_AZURE_CONTAINER").unwrap_or_else(|_| "rdd-test".to_owned());
    let storage =
        AzureBlobStorage::new(AzureBlobConfig::new(account, key, container).endpoint(url)).unwrap();
    round_trip(&storage, test_key()).await;
}

#[cfg(feature = "upload-gcs")]
#[tokio::test]
async fn u07_round_trip_gcs_si_hay_emulador() {
    use rdd::upload::{GcsConfig, GcsStorage};

    let Ok(url) = std::env::var("RDD_TEST_GCS_URL") else {
        return;
    };
    let bucket = std::env::var("RDD_TEST_GCS_BUCKET").unwrap_or_else(|_| "rdd-test".to_owned());
    let access = std::env::var("RDD_TEST_GCS_ACCESS_ID").unwrap_or_else(|_| "fake".to_owned());
    let secret = std::env::var("RDD_TEST_GCS_SECRET").unwrap_or_else(|_| "fake".to_owned());
    let storage = GcsStorage::new(GcsConfig::new(bucket, access, secret).endpoint(url)).unwrap();
    round_trip(&storage, test_key()).await;
}
