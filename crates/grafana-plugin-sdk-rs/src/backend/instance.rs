/*! Instance management for plugins.

A plugin process serves every instance of the plugin configured across a Grafana
server. An [`InstanceManager`] lazily creates one instance per unique
configuration, caches it, and recreates it when the configuration changes,
disposing of the previous instance. This mirrors the Go SDK's `instancemgmt`.

Provide an [`InstanceProvider`] describing how to key, validate and build your
instance; instances may implement [`Instance::dispose`] to release resources
(database pools, HTTP clients, …) when they are evicted.

For app plugins the standard behaviour — key by `plugin_id`/`org_id` and recreate
when the settings' update time or the Grafana configuration change — is available
through the [`app_instance_key`] and [`app_needs_update`] helpers.
*/
use std::{collections::HashMap, fmt::Debug, hash::Hash, sync::Arc, time::Duration};

use serde::de::DeserializeOwned;
use tokio::sync::RwLock;

use super::{AppInstanceSettings, PluginContext};

/// The default delay before an evicted instance is disposed of, giving in-flight
/// requests time to complete. Matches the Go SDK.
const DEFAULT_DISPOSE_TTL: Duration = Duration::from_secs(5);

/// A managed plugin instance.
///
/// Implement [`dispose`](Instance::dispose) to release resources held by the
/// instance when it is evicted from its [`InstanceManager`] (because its
/// configuration changed). The default implementation does nothing.
pub trait Instance: Send + Sync {
    /// Called when this instance is evicted, after the manager's dispose delay.
    fn dispose(&self) {}
}

/// Describes how an [`InstanceManager`] keys, validates and creates instances.
#[tonic::async_trait]
pub trait InstanceProvider: Send + Sync {
    /// The context used to create instances, typically a [`PluginContext`].
    type Context: Clone + Send + Sync + 'static;
    /// The key uniquely identifying an instance in the cache.
    type CacheKey: Eq + Hash + Clone + Send + Sync + 'static;
    /// The instance type produced.
    type Instance: Instance + 'static;
    /// The error returned when a key cannot be derived or an instance cannot be created.
    type Error: Send;

    /// Derive the cache key for the given context.
    fn cache_key(&self, context: &Self::Context) -> Result<Self::CacheKey, Self::Error>;

    /// Return `true` if the cached instance created from `cached` is stale for
    /// the `current` context and should be recreated.
    fn needs_update(&self, current: &Self::Context, cached: &Self::Context) -> bool;

    /// Create a new instance for the given context.
    async fn new_instance(&self, context: &Self::Context) -> Result<Self::Instance, Self::Error>;
}

struct Cached<P: InstanceProvider> {
    context: P::Context,
    instance: Arc<P::Instance>,
}

/// Caches one plugin instance per unique configuration, recreating and disposing
/// instances as the configuration changes.
///
/// Cheap to share behind an [`Arc`]; [`get`](InstanceManager::get) is safe to call
/// concurrently.
pub struct InstanceManager<P: InstanceProvider> {
    provider: P,
    dispose_ttl: Duration,
    cache: RwLock<HashMap<P::CacheKey, Cached<P>>>,
}

impl<P: InstanceProvider> InstanceManager<P> {
    /// Create a new manager backed by `provider`.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            dispose_ttl: DEFAULT_DISPOSE_TTL,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Set how long to wait after an instance is evicted before disposing of it.
    #[must_use]
    pub fn with_dispose_ttl(mut self, ttl: Duration) -> Self {
        self.dispose_ttl = ttl;
        self
    }

    /// Return the instance for `context`, creating it if necessary.
    ///
    /// If a cached instance exists and is still current it is returned; otherwise a
    /// new instance is created, cached, and any previous instance is scheduled for
    /// disposal.
    pub async fn get(&self, context: &P::Context) -> Result<Arc<P::Instance>, P::Error> {
        let key = self.provider.cache_key(context)?;

        // Fast path: an up-to-date instance already exists.
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&key) {
                if !self.provider.needs_update(context, &cached.context) {
                    return Ok(Arc::clone(&cached.instance));
                }
            }
        }

        // Slow path: create (double-checking under the write lock).
        let mut cache = self.cache.write().await;
        if let Some(cached) = cache.get(&key) {
            if !self.provider.needs_update(context, &cached.context) {
                return Ok(Arc::clone(&cached.instance));
            }
        }
        let instance = Arc::new(self.provider.new_instance(context).await?);
        let previous = cache.insert(
            key,
            Cached {
                context: context.clone(),
                instance: Arc::clone(&instance),
            },
        );
        drop(cache);
        if let Some(previous) = previous {
            self.schedule_dispose(previous.instance);
        }
        Ok(instance)
    }

    fn schedule_dispose(&self, instance: Arc<P::Instance>) {
        let ttl = self.dispose_ttl;
        tokio::spawn(async move {
            tokio::time::sleep(ttl).await;
            instance.dispose();
        });
    }
}

/// The standard cache key for an app plugin instance: `"{plugin_id}#{org_id}"`.
///
/// Useful when implementing [`InstanceProvider::cache_key`] for an app plugin.
pub fn app_instance_key<J, S>(context: &PluginContext<AppInstanceSettings<J, S>, J, S>) -> String
where
    J: Debug + DeserializeOwned + Send + Sync,
    S: DeserializeOwned + Send + Sync,
{
    format!("{}#{}", context.plugin_id, context.org_id)
}

/// The standard freshness check for an app plugin instance.
///
/// Returns `true` when the instance settings' update time or the Grafana
/// configuration differ between `current` and `cached`, meaning the instance
/// should be recreated. Useful when implementing [`InstanceProvider::needs_update`].
pub fn app_needs_update<J, S>(
    current: &PluginContext<AppInstanceSettings<J, S>, J, S>,
    cached: &PluginContext<AppInstanceSettings<J, S>, J, S>,
) -> bool
where
    J: Debug + DeserializeOwned + Send + Sync,
    S: DeserializeOwned + Send + Sync,
{
    let current_updated = current.instance_settings.as_ref().map(|s| s.updated);
    let cached_updated = cached.instance_settings.as_ref().map(|s| s.updated);
    current_updated != cached_updated || current.grafana_config != cached.grafana_config
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    #[derive(Clone)]
    struct Ctx {
        key: String,
        version: u64,
    }

    struct TestInstance {
        disposed: Arc<AtomicUsize>,
    }
    impl Instance for TestInstance {
        fn dispose(&self) {
            self.disposed.fetch_add(1, SeqCst);
        }
    }

    struct TestProvider {
        created: Arc<AtomicUsize>,
        disposed: Arc<AtomicUsize>,
    }

    #[tonic::async_trait]
    impl InstanceProvider for TestProvider {
        type Context = Ctx;
        type CacheKey = String;
        type Instance = TestInstance;
        type Error = std::convert::Infallible;

        fn cache_key(&self, context: &Ctx) -> Result<String, Self::Error> {
            Ok(context.key.clone())
        }
        fn needs_update(&self, current: &Ctx, cached: &Ctx) -> bool {
            current.version != cached.version
        }
        async fn new_instance(&self, _context: &Ctx) -> Result<TestInstance, Self::Error> {
            self.created.fetch_add(1, SeqCst);
            Ok(TestInstance {
                disposed: Arc::clone(&self.disposed),
            })
        }
    }

    #[tokio::test]
    async fn caches_recreates_and_disposes() {
        let created = Arc::new(AtomicUsize::new(0));
        let disposed = Arc::new(AtomicUsize::new(0));
        let manager = InstanceManager::new(TestProvider {
            created: Arc::clone(&created),
            disposed: Arc::clone(&disposed),
        })
        .with_dispose_ttl(Duration::ZERO);

        let v1 = Ctx {
            key: "app#1".to_owned(),
            version: 1,
        };
        let a = manager.get(&v1).await.unwrap();
        let b = manager.get(&v1).await.unwrap();
        // Same configuration -> same instance, created once.
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(created.load(SeqCst), 1);

        // Changed configuration -> new instance; the old one is disposed.
        let v2 = Ctx {
            key: "app#1".to_owned(),
            version: 2,
        };
        let c = manager.get(&v2).await.unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(created.load(SeqCst), 2);

        // Disposal is spawned; wait briefly for it to run.
        for _ in 0..50 {
            if disposed.load(SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(disposed.load(SeqCst), 1);

        // A different key is an independent instance.
        let other = Ctx {
            key: "app#2".to_owned(),
            version: 1,
        };
        manager.get(&other).await.unwrap();
        assert_eq!(created.load(SeqCst), 3);
    }
}
