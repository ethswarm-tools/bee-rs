//! Generic API surface: upload/download options, headers, rich
//! return types, and the pin / tag / stewardship endpoint methods.
//! Mirrors `pkg/api` in bee-go.

pub mod endpoints;
pub mod options;
pub mod result;

pub use endpoints::{ApiService, EnvelopeResponse, GranteeResponse, PinIntegrity, Tag};
pub use options::{
    CollectionUploadOptions, DownloadOptions, FileUploadOptions, HeaderPairs, OnEntryFn,
    PostageBatchOptions, RedundancyLevel, RedundancyStrategy, RedundantUploadOptions,
    UploadOptions, UploadProgress, prepare_collection_upload_headers, prepare_download_headers,
    prepare_file_upload_headers, prepare_redundant_upload_headers, prepare_upload_headers,
    validate_collection_upload_options,
};
pub use result::{FileHeaders, UploadResult, parse_content_disposition_filename};
