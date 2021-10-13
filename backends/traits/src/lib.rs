//
// Copyright (c) 2017, 2020 ADLINK Technology Inc.
//
// This program and the accompanying materials are made available under the
// terms of the Eclipse Public License 2.0 which is available at
// http://www.eclipse.org/legal/epl-2.0, or the Apache License, Version 2.0
// which is available at https://www.apache.org/licenses/LICENSE-2.0.
//
// SPDX-License-Identifier: EPL-2.0 OR Apache-2.0
//
// Contributors:
//   ADLINK zenoh team, <zenoh@adlink-labs.tech>
//

//! This crate provides the traits to be implemented by a zenoh backend library:
//!  - [`Backend`]
//!  - [`Storage`]
//!
//! Such library must also declare a `create_backend()` operation
//! with the `#[no_mangle]` attribute as an entrypoint to be called for the Backend creation.
//!
//! # Example
//! ```
//! use async_trait::async_trait;
//! use zenoh::prelude::*;
//! use zenoh::properties::properties_to_json_value;
//! use zenoh_backend_traits::*;
//!
//! #[no_mangle]
//! pub fn create_backend(properties: &Properties) -> ZResult<Box<dyn Backend>> {
//!     // The properties are the ones passed via a PUT in the admin space for Backend creation.
//!     // Here we re-expose them in the admin space for GET operations, adding the PROP_BACKEND_TYPE entry.
//!     let mut p = properties.clone();
//!     p.insert(PROP_BACKEND_TYPE.into(), "my_backend_type".into());
//!     let admin_status = properties_to_json_value(&p);
//!     Ok(Box::new(MyBackend { admin_status }))
//! }
//!
//! // Your Backend implementation
//! struct MyBackend {
//!     admin_status: Value,
//! }
//!
//! #[async_trait]
//! impl Backend for MyBackend {
//!     async fn get_admin_status(&self) -> Value {
//!         // This operation is called on GET operation on the admin space for the Backend
//!         // Here we reply with a static status (containing the configuration properties).
//!         // But we could add dynamic properties for Backend monitoring.
//!         self.admin_status.clone()
//!     }
//!
//!     async fn create_storage(&mut self, properties: Properties) -> ZResult<Box<dyn Storage>> {
//!         // The properties are the ones passed via a PUT in the admin space for Storage creation.
//!         Ok(Box::new(MyStorage::new(properties).await?))
//!     }
//!
//!     fn incoming_data_interceptor(&self) -> Option<Box<dyn IncomingDataInterceptor>> {
//!         // No interception point for incoming data (on PUT operations)
//!         None
//!     }
//!
//!     fn outgoing_data_interceptor(&self) -> Option<Box<dyn OutgoingDataInterceptor>> {
//!         // No interception point for outgoing data (on GET operations)
//!         None
//!     }
//! }
//!
//! // Your Storage implementation
//! struct MyStorage {
//!     admin_status: Value,
//! }
//!
//! impl MyStorage {
//!     async fn new(properties: Properties) -> ZResult<MyStorage> {
//!         // The properties are the ones passed via a PUT in the admin space for Storage creation.
//!         // They contain at least a PROP_STORAGE_KEY_EXPR entry (i.e. "key_expr").
//!         // Here we choose to re-expose them as they are in the admin space for GET operations.
//!         let admin_status = properties_to_json_value(&properties);
//!         Ok(MyStorage { admin_status })
//!     }
//! }
//!
//! #[async_trait]
//! impl Storage for MyStorage {
//!     async fn get_admin_status(&self) -> Value {
//!         // This operation is called on GET operation on the admin space for the Storage
//!         // Here we reply with a static status (containing the configuration properties).
//!         // But we could add dynamic properties for Storage monitoring.
//!         self.admin_status.clone()
//!     }
//!
//!     async fn on_sample(&mut self, mut sample: Sample) -> ZResult<()> {
//!         // When receiving a Sample (i.e. on PUT or DELETE operations)
//!         // extract Timestamp from sample
//!         sample.ensure_timestamp();
//!         let timestamp = sample.timestamp.take().unwrap();
//!         // Store or delete the sample depending the ChangeKind
//!         match sample.kind {
//!             SampleKind::Put => {
//!                 let _key = sample.res_key;
//!                 // @TODO:
//!                 //  - check if timestamp is newer than the stored one for the same key
//!                 //  - if yes: store (key, sample)
//!                 //  - if not: drop the sample
//!             }
//!             SampleKind::Delete => {
//!                 let _key = sample.res_key;
//!                 // @TODO:
//!                 //  - check if timestamp is newer than the stored one for the same key
//!                 //  - if yes: mark key as deleted (possibly scheduling definitive removal for later)
//!                 //  - if not: drop the sample
//!             }
//!             SampleKind::Patch => {
//!                 println!("Received PATCH for {}: not yet supported", sample.res_key);
//!             }
//!         }
//!         Ok(())
//!     }
//!
//!     // When receiving a Query (i.e. on GET operations)
//!     async fn on_query(&mut self, query: Query) -> ZResult<()> {
//!         let _key_elector = query.key_selector();
//!         // @TODO:
//!         //  - test if key selector contains *
//!         //  - if not: just get the sample with key==key_selector and call: query.reply(sample.clone()).await;
//!         //  - if yes: get all the samples with key matching key_selector and call for each: query.reply(sample.clone()).await;
//!         //
//!         // NOTE: in case query.value_selector() is not empty something smarter should be done with returned samples...
//!         Ok(())
//!     }
//! }
//! ```

use async_std::sync::{Arc, RwLock};
use async_trait::async_trait;
use zenoh::prelude::*;

pub mod utils;

/// The `"type"` property key to be used in admin status reported by Backends.
pub const PROP_BACKEND_TYPE: &str = "type";

/// The `"key_expr"` property key to be used for configuration of each storage.
pub const PROP_STORAGE_KEY_EXPR: &str = "key_expr";

/// The `"key_prefix"` property key that could be used to specify the common key prefix
/// to be stripped from keys before storing them as keys in the Storage.
///
/// Note that it shall be a prefix of the `"key_expr"`.
/// If you use it, you should also adapt in [`Storage::on_query()`] implementation the incoming
/// queries' key expression to the stored keys calling [`crate::utils::get_sub_key_selectors()`].
pub const PROP_STORAGE_KEY_PREFIX: &str = "key_prefix";

/// Trait to be implemented by a Backend.
///
#[async_trait]
pub trait Backend: Send + Sync {
    /// Returns the status that will be sent as a reply to a query
    /// on the administration space for this backend.
    async fn get_admin_status(&self) -> Value;

    /// Creates a storage configured with some properties.
    async fn create_storage(&mut self, props: Properties) -> ZResult<Box<dyn Storage>>;

    /// Returns an [`IncomingDataInterceptor`] that will be called before pushing any data
    /// into a storage created by this backend. `None` can be returned for no interception point.
    fn incoming_data_interceptor(&self) -> Option<Box<dyn IncomingDataInterceptor>>;

    /// Returns an [`OutgoingDataInterceptor`] that will be called before sending any reply
    /// to a query from a storage created by this backend. `None` can be returned for no interception point.
    fn outgoing_data_interceptor(&self) -> Option<Box<dyn OutgoingDataInterceptor>>;
}

/// Trait to be implemented by a Storage.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Returns the status that will be sent as a reply to a query
    /// on the administration space for this storage.
    async fn get_admin_status(&self) -> Value;

    /// Function called for each incoming data ([`Sample`]) to be stored in this storage.
    async fn on_sample(&mut self, sample: Sample) -> ZResult<()>;

    /// Function called for each incoming query matching this storage's keys exp.
    /// This storage should reply with data matching the query calling [`Query::reply()`].
    async fn on_query(&mut self, query: Query) -> ZResult<()>;
}

/// An interceptor allowing to modify the data pushed into a storage before it's actually stored.
#[async_trait]
pub trait IncomingDataInterceptor: Send + Sync {
    async fn on_sample(&self, sample: Sample) -> Sample;
}

/// An interceptor allowing to modify the data going out of a storage before it's sent as a reply to a query.
#[async_trait]
pub trait OutgoingDataInterceptor: Send + Sync {
    async fn on_reply(&self, sample: Sample) -> Sample;
}

/// A wrapper around the [`zenoh::queryable::Query`] allowing to call the
/// OutgoingDataInterceptor (if any) before to send the reply
pub struct Query {
    q: zenoh::queryable::Query,
    interceptor: Option<Arc<RwLock<Box<dyn OutgoingDataInterceptor>>>>,
}

impl Query {
    pub fn new(
        q: zenoh::queryable::Query,
        interceptor: Option<Arc<RwLock<Box<dyn OutgoingDataInterceptor>>>>,
    ) -> Query {
        Query { q, interceptor }
    }

    /// The full [`Selector`] of this Query.
    #[inline(always)]
    pub fn selector(&self) -> Selector<'_> {
        self.q.selector()
    }

    /// The key selector part of this Query.
    #[inline(always)]
    pub fn key_selector(&self) -> &ResKey<'_> {
        self.q.key_selector()
    }

    /// The value selector part of this Query.
    #[inline(always)]
    pub fn value_selector(&self) -> &str {
        self.q.value_selector()
    }

    /// Sends a Sample as a reply to this Query
    pub async fn reply(&self, sample: Sample) {
        // Call outgoing intercerceptor
        let sample = if let Some(ref interceptor) = self.interceptor {
            interceptor.read().await.on_reply(sample).await
        } else {
            sample
        };
        // Send reply
        self.q.reply_async(sample).await
    }
}
