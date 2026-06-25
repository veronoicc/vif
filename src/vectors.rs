use std::{collections::HashMap, str::FromStr, sync::Arc, vec};

use qdrant_client::{
    Payload, Qdrant, QdrantError,
    qdrant::{
        CollectionExistsRequest, Condition, CreateCollectionBuilder, Filter, GetPointsBuilder, NamedVectors, PointId, PointStruct, ScrollPointsBuilder,
        SetPayloadPointsBuilder, UpsertPointsBuilder, Value, Vectors, VectorsConfigBuilder,
        value::Kind, vector_output::Vector as VectorOutputVector, vectors::VectorsOptions,
        vectors_output::VectorsOptions as VectorsOutputOptions,
    },
};
use serde_json::json;
use uuid::Uuid;

use crate::embedding::Embedder;

pub async fn initialize(
    qdrant: &Qdrant,
    embedders: &[Arc<dyn Embedder + Send + Sync>],
) -> Result<(), QdrantError> {
    let mut vectors = VectorsConfigBuilder::default();
    for embedder in embedders {
        vectors.add_named_vector_params(embedder.name(), embedder.params());
    }

    if !qdrant
        .collection_exists(CollectionExistsRequest {
            collection_name: "media".into(),
        })
        .await?
    {
        qdrant
            .create_collection(
                CreateCollectionBuilder::new("media").vectors_config(vectors.clone()),
            )
            .await?;
    }

    if !qdrant
        .collection_exists(CollectionExistsRequest {
            collection_name: "queries".into(),
        })
        .await?
    {
        qdrant
            .create_collection(CreateCollectionBuilder::new("queries").vectors_config(vectors))
            .await?;
    }

    Ok(())
}

pub async fn get_media(
    qdrant: &Qdrant,
    uuid: &Uuid,
) -> Result<Option<(HashMap<String, Vec<f32>>, String, Vec<Uuid>)>, QdrantError> {
    let response = qdrant
        .get_points(
            GetPointsBuilder::new("media", vec![PointId::from(uuid.to_string())])
                .with_payload(true)
                .with_vectors(true),
        )
        .await?;

    if let Some(point) = response.result.first() {
        let Some(VectorsOutputOptions::Vectors(output)) = point
            .vectors
            .as_ref()
            .and_then(|v| v.vectors_options.as_ref())
        else {
            return Ok(None);
        };

        let vectors_map = output
            .vectors
            .iter()
            .map(|(name, vector_output)| (name.clone(), vector_output.data.clone()))
            .collect::<HashMap<String, Vec<f32>>>();

        let Some(link) = point.payload.get("link").map(|v| v.to_string()) else {
            return Ok(None);
        };

        let Some(users) = point
            .payload
            .get("users")
            .and_then(|v| v.as_list())
            .map(|list| {
                list.iter()
                    .map(|val| val.as_str().and_then(|s| s.parse::<Uuid>().ok()))
                    .collect::<Option<Vec<Uuid>>>()
            })
            .flatten()
        else {
            return Ok(None);
        };

        return Ok(Some((vectors_map, link, users)));
    }

    Ok(None)
}

pub async fn set_media_users(
    qdrant: &Qdrant,
    uuid: &Uuid,
    users: &[Uuid],
) -> Result<(), QdrantError> {
    let payload: Payload = json!({"users": users}).try_into().unwrap();

    qdrant
        .set_payload(
            SetPayloadPointsBuilder::new("media", payload)
                .points_selector(vec![PointId::from(uuid.to_string())])
                .wait(true),
        )
        .await?;

    Ok(())
}

pub async fn insert_media(
    qdrant: &Qdrant,
    uuid: &Uuid,
    vectors: &HashMap<String, Vec<f32>>,
    link: &str,
    user: &Uuid,
) -> Result<(), QdrantError> {
    let users = vec![*user]
        .into_iter()
        .map(|uuid| Value::from(uuid.to_string()))
        .collect::<Vec<_>>();
    let mut payload = HashMap::new();
    payload.insert("link".to_string(), Value::from(link));
    payload.insert("users".to_string(), users.into());

    let mut named_vectors = NamedVectors::default();
    for (name, vector) in vectors {
        named_vectors = named_vectors.add_vector(name, vector.clone());
    }

    qdrant
        .upsert_points(UpsertPointsBuilder::new(
            "media",
            vec![PointStruct {
                id: Some(PointId::from(uuid.to_string())),
                payload,
                vectors: Some(Vectors {
                    vectors_options: Some(VectorsOptions::Vectors(named_vectors)),
                }),
            }],
        ))
        .await?;

    Ok(())
}

pub async fn insert_query(
    qdrant: &Qdrant,
    uuid: &Uuid,
    vectors: &HashMap<String, Vec<f32>>,
    query: &str,
) -> Result<(), QdrantError> {
    let mut payload = HashMap::new();
    payload.insert("text".to_string(), Value::from(query));

    let mut named_vectors = NamedVectors::default();
    for (name, vector) in vectors {
        named_vectors = named_vectors.add_vector(name, vector.clone());
    }

    qdrant
        .upsert_points(UpsertPointsBuilder::new(
            "queries",
            vec![PointStruct {
                id: Some(PointId::from(uuid.to_string())),
                payload,
                vectors: Some(Vectors {
                    vectors_options: Some(VectorsOptions::Vectors(named_vectors)),
                }),
            }],
        ))
        .await?;

    Ok(())
}

pub async fn get_query(
    qdrant: &Qdrant,
    uuid: &Uuid,
) -> Result<Option<(HashMap<String, Vec<f32>>, String)>, QdrantError> {
    let response = qdrant
        .get_points(
            GetPointsBuilder::new("queries", vec![PointId::from(uuid.to_string())])
                .with_payload(true),
        )
        .await?;

    if let Some(point) = response.result.first() {
        let Some(VectorsOutputOptions::Vectors(output)) = point
            .vectors
            .as_ref()
            .and_then(|v| v.vectors_options.as_ref())
        else {
            return Ok(None);
        };

        let vectors_map = output
            .vectors
            .iter()
            .filter_map(|(name, vector)| {
                let Some(VectorOutputVector::Dense(dense)) = &vector.vector else {
                    return None;
                };
                Some((name.clone(), dense.data.clone()))
            })
            .collect::<HashMap<String, Vec<f32>>>();

        let Some(link) = point.payload.get("text").map(|v| v.to_string()) else {
            return Ok(None);
        };

        return Ok(Some((vectors_map, link)));
    }

    Ok(None)
}

pub async fn get_links(user: &Uuid, qdrant: &Qdrant) -> Result<HashMap<Uuid, String>, QdrantError> {
    let mut links = HashMap::new();
    let mut offset = None;

    let filter = Filter::must([Condition::matches("users", user.to_string())]);

    loop {
        let mut builder = ScrollPointsBuilder::new("media")
            .with_payload(true)
            .filter(filter.clone())
            .limit(100);

        if let Some(next_offset) = offset {
            builder = builder.offset(next_offset);
        }

        let response = qdrant.scroll(builder).await?;

        if response.result.is_empty() {
            break;
        }

        links.extend(response.result.into_iter().filter_map(|point| {
            let id_options = point.id?.point_id_options?;

            let uuid_str = match id_options {
                qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u) => u,
                _ => return None,
            };

            let uuid = Uuid::from_str(&uuid_str).ok()?;
            let link = point
                .payload
                .get("link")
                .and_then(|val| val.kind.as_ref())
                .and_then(|kind| match kind {
                    Kind::StringValue(s) => Some(s.clone()),
                    _ => None,
                })?;
            Some((uuid, link))
        }));

        offset = response.next_page_offset;
        if offset.is_none() {
            break;
        }
    }

    Ok(links)
}
