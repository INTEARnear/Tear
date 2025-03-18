use std::{
    fmt::Debug,
    future::Future,
    hash::Hash,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use dashmap::{mapref::multiple::RefMulti, DashMap};
use futures_util::{lock::Mutex, TryStreamExt};
use mongodb::{
    error::{ErrorKind, WriteError, WriteFailure},
    Database, IndexModel,
};
use serde::{Deserialize, Serialize};

// TODO refactor this hell, it's not even properly synchronized

// Why mongodb? Because the old bot used it and I was delusional enough to think that
// I can migrate the data from the old bot to the new one and it would be easier.
//
// Why mongodb on the old bot? Because I messed up mariadb on my machine and it was easier
// to just go with whatever I already have set up and works, the bot wasn't meant to be a long-term
// project.
//
// It might be a good idea to rewrite the cache to another key-value store.

/// A store that caches values in memory and persists them in a MongoDB collection.
///
/// It takes advantage of the in-memory cache to reduce the number of reads from the database.
/// Sometimes the cache can be fully stored in memory (e.g. all known tokens, it's not like
/// there are millions of them), in this case the `cached_all` flag is set to true and the store
/// will not read from the database if the key is not found in the cache.
///
/// This structure does not allow writing to the underlying connection outside of
/// the program, or using multiple instances of the same collection.
pub struct PersistentCachedStore<
    K: Serialize + Clone + Send + Sync + Unpin + 'static + Eq + Hash,
    V: Serialize + Clone + Send + Sync + Unpin + 'static,
> {
    locks: DashMap<K, Arc<Mutex<()>>>,
    cache: DashMap<K, V>,
    db: mongodb::Collection<CacheEntry<K, V>>,
    cached_all: AtomicBool,
}

impl<
        K: Serialize + Clone + Send + Sync + Unpin + 'static + Eq + Hash,
        V: Serialize + Clone + Send + Sync + Unpin + 'static,
    > Debug for PersistentCachedStore<K, V>
where
    CacheEntry<K, V>: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentCachedStore")
            .field("cache", &self.cache.len())
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheEntry<
    K: Serialize + Clone + Send + Sync + Unpin + 'static + Eq + Hash,
    V: Serialize + Clone + Send + Sync + Unpin + 'static,
> {
    key: K,
    value: V,
}

impl<
        K: Serialize + Clone + Send + Sync + Unpin + 'static + Eq + Hash,
        V: Serialize + Clone + Send + Sync + Unpin + 'static,
    > PersistentCachedStore<K, V>
where
    CacheEntry<K, V>: Serialize + for<'de> Deserialize<'de>,
{
    pub async fn new(db: Database, name: &str) -> Result<Self, anyhow::Error> {
        let cache = DashMap::<K, V>::new();
        let collection = db.collection(name);
        collection
            .create_index(
                IndexModel::builder()
                    .keys(bson::doc! { "key": 1 })
                    .options(
                        mongodb::options::IndexOptions::builder()
                            .unique(true)
                            .build(),
                    )
                    .build(),
            )
            .await?;
        Ok(Self {
            locks: cache
                .iter()
                .map(|e| (e.key().clone(), Arc::new(Mutex::new(()))))
                .collect(),
            cache,
            db: collection,
            cached_all: AtomicBool::new(false),
        })
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        if let Some(value) = self.cache.get(key).as_deref() {
            return Some(value.clone());
        }
        match bson::to_bson(key) {
            Ok(key_bson) => {
                let value = self
                    .db
                    .find_one(bson::doc! { "key": key_bson })
                    .await
                    .map_err(|e| log::error!("Error getting cache entry: {:?}", e))
                    .unwrap_or(None)
                    .map(|entry| entry.value);
                if let Some(value) = value.as_ref() {
                    self.cache.insert(key.clone(), value.clone());
                }
                value
            }
            Err(e) => {
                log::error!("Error serializing key: {:?}", e);
                None
            }
        }
    }

    pub async fn insert_if_not_exists(&self, key: K, value: V) -> Result<bool, anyhow::Error> {
        if self.cache.contains_key(&key) {
            return Ok(false);
        }
        if let Err(err) = self
            .db
            .insert_one(CacheEntry {
                key: key.clone(),
                value: value.clone(),
            })
            .await
        {
            if let ErrorKind::Write(WriteFailure::WriteError(WriteError { code: 11000, .. })) =
                &*err.kind
            {
                return Ok(false);
            }
            Err(err.into())
        } else {
            self.cache.insert(key, value);
            Ok(true)
        }
    }

    /// Edits the value of the key, and returns the result of the edit function.
    pub async fn edit<R>(
        &self,
        key: K,
        edit: impl FnOnce(&mut V) -> R,
        default: Option<V>,
    ) -> Result<R, anyhow::Error> {
        let lock = self
            .locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())));
        let guard = lock.lock().await;
        let _ = self.get(&key).await;
        let mut value = self
            .cache
            .get(&key)
            .map(|e| e.value().clone())
            .or(default)
            .ok_or_else(|| anyhow::anyhow!("No value found for key"))?;
        let r = edit(&mut value);
        self.insert_or_update(key, value).await?;
        drop(guard);
        Ok(r)
    }

    /// Edits the value of the key, and returns the result of the edit function.
    pub async fn edit_async<R, F>(
        &self,
        key: K,
        edit: impl FnOnce(V) -> F,
        default: Option<V>,
    ) -> Result<Option<R>, anyhow::Error>
    where
        F: Future<Output = Option<(V, R)>>,
    {
        let lock = self
            .locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())));
        let guard = lock.lock().await;
        let _ = self.get(&key).await;
        let value = self
            .cache
            .get(&key)
            .map(|e| e.value().clone())
            .or(default)
            .ok_or_else(|| anyhow::anyhow!("No value found for key"))?;
        let r = if let Some((new_value, r)) = edit(value).await {
            self.insert_or_update(key, new_value).await?;
            Some(r)
        } else {
            None
        };
        drop(guard);
        Ok(r)
    }

    /// Edits the value of the key, if the edit function returns `Some`, returns
    /// it and saves the new value. If the edit function returns `None`, returns
    /// `None` and does not save the new value.
    pub async fn edit_optional<R>(
        &self,
        key: K,
        edit: impl FnOnce(&mut V) -> Option<R>,
        default: Option<V>,
    ) -> Result<Option<R>, anyhow::Error> {
        let lock = self
            .locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())));
        let guard = lock.lock().await;
        let _ = self.get(&key).await;
        let mut value = self
            .cache
            .get(&key)
            .map(|e| e.value().clone())
            .or(default)
            .ok_or_else(|| anyhow::anyhow!("No value found for key"))?;
        let r = edit(&mut value);
        self.insert_or_update(key, value).await?;
        drop(guard);
        Ok(r)
    }

    pub async fn insert_or_update(&self, key: K, value: V) -> Result<(), anyhow::Error> {
        self.cache.insert(key.clone(), value.clone());
        let key_bson = bson::to_bson(&key)?;
        let value_bson = bson::to_bson(&value)?;
        self.db
            .update_one(
                bson::doc! { "key": key_bson },
                bson::doc! { "$set": bson::doc! { "value": value_bson } },
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn remove(&self, key: &K) -> Result<Option<V>, anyhow::Error> {
        let removed = self.cache.remove(key);
        if self.cached_all.load(Ordering::Relaxed) && removed.is_none() {
            return Ok(None);
        }
        if let Some(value) = self.get(key).await {
            let key_bson = bson::to_bson(key)?;
            self.db.delete_one(bson::doc! { "key": key_bson }).await?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    pub async fn values(&self) -> Result<impl Iterator<Item = RefMulti<K, V>>, anyhow::Error> {
        if !self.cached_all.load(Ordering::Relaxed) {
            let mut cursor = self.db.find(bson::doc! {}).await?;
            while let Some(result) = cursor.try_next().await? {
                self.cache.insert(result.key.clone(), result.value.clone());
            }
            self.cached_all.store(true, Ordering::Relaxed);
        }
        Ok(self.cache.iter())
    }

    pub async fn contains_key(&self, key: &K) -> Result<bool, anyhow::Error> {
        if self.cached_all.load(Ordering::Relaxed) {
            Ok(self.cache.contains_key(key))
        } else {
            if self.cache.contains_key(key) {
                return Ok(true);
            }
            Ok(self
                .db
                .find_one(bson::doc! { "key": bson::to_bson(key)? })
                .await?
                .is_some())
        }
    }

    pub async fn delete_many(
        &self,
        keys: impl IntoIterator<Item = K>,
    ) -> Result<(), anyhow::Error> {
        let keys: Vec<K> = keys.into_iter().collect();
        for key in keys.iter() {
            self.cache.remove(key);
        }
        let keys_bson: Vec<bson::Bson> =
            keys.iter().map(|key| bson::to_bson(key).unwrap()).collect();
        self.db
            .delete_many(bson::doc! { "key": { "$in": keys_bson } })
            .await?;
        Ok(())
    }
}

pub struct PersistentUncachedStore<
    K: Serialize + Clone + Send + Sync + Unpin + 'static + Eq + Hash,
    V: Serialize + Clone + Send + Sync + Unpin + 'static,
> {
    db: mongodb::Collection<CacheEntry<K, V>>,
}

impl<
        K: Serialize + Clone + Send + Sync + Unpin + 'static + Eq + Hash,
        V: Serialize + Clone + Send + Sync + Unpin + 'static,
    > PersistentUncachedStore<K, V>
where
    CacheEntry<K, V>: Serialize + for<'de> Deserialize<'de>,
{
    pub async fn new(db: Database, name: &str) -> Result<Self, anyhow::Error> {
        let collection = db.collection(name);
        collection
            .create_index(
                IndexModel::builder()
                    .keys(bson::doc! { "key": 1 })
                    .options(
                        mongodb::options::IndexOptions::builder()
                            .unique(true)
                            .build(),
                    )
                    .build(),
            )
            .await?;
        Ok(Self { db: collection })
    }

    pub async fn get(&self, key: &K) -> Option<V> {
        match bson::to_bson(key) {
            Ok(key_bson) => self
                .db
                .find_one(bson::doc! { "key": key_bson })
                .await
                .map_err(|e| log::error!("Error getting cache entry: {:?}", e))
                .unwrap_or(None)
                .map(|entry| entry.value),
            Err(e) => {
                log::error!("Error serializing key: {:?}", e);
                None
            }
        }
    }

    pub async fn insert_or_update(&self, key: K, value: V) -> Result<(), anyhow::Error> {
        let key_bson = bson::to_bson(&key)?;
        let value_bson = bson::to_bson(&value)?;
        self.db
            .update_one(
                bson::doc! { "key": key_bson },
                bson::doc! { "$set": bson::doc! { "value": value_bson } },
            )
            .upsert(true)
            .await?;
        Ok(())
    }

    pub async fn remove(&self, key: &K) -> Result<(), anyhow::Error> {
        let key_bson = bson::to_bson(key)?;
        self.db.delete_one(bson::doc! { "key": key_bson }).await?;
        Ok(())
    }

    pub async fn values(&self) -> Result<impl Iterator<Item = (K, V)>, anyhow::Error> {
        let mut cursor = self.db.find(bson::doc! {}).await?;
        let mut vec = Vec::new();
        while let Some(result) = cursor.try_next().await? {
            vec.push((result.key, result.value));
        }
        Ok(vec.into_iter())
    }
}
