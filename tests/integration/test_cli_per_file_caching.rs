// Integration test to validate CLI per-file indexing with caching behavior
// This test catches the production bug where cached files skip relationship collection
//
// Context: Issue #24 revealed that the integration test (batch API via index_directory)
// gave false confidence by passing with 580 relationships, while production CLI
// (per-file via index_file_with_force) failed with 0 relationships due to caching.

use codanna::indexing::{SimpleIndexer, IndexStats};
use codanna::config::Settings;
use codanna::parsing::ruby::RubyParser;
use codanna::parsing::LanguageParser;
use std::path::PathBuf;
use tempfile::TempDir;
use std::fs;
use std::sync::Arc;

/// Test that simulates the production CLI workflow:
/// 1. Initial indexing (creates cache)
/// 2. Re-indexing without --force (should use cache but still collect relationships)
/// 3. Re-indexing with --force (should bypass cache and re-collect relationships)
#[test]
fn test_cli_per_file_caching_preserves_relationships() {
    // Setup: Create temporary test directory with Ruby files containing relationships
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let test_dir = temp_dir.path();

    // Create a simple Rails-like structure with singleton method calls
    let models_dir = test_dir.join("app/models");
    fs::create_dir_all(&models_dir).expect("Failed to create models dir");

    // File 1: UrlFormatter module with singleton method
    let formatter_file = models_dir.join("url_formatter.rb");
    fs::write(
        &formatter_file,
        r#"
module UrlFormatter
  def self.display_url(url)
    url.to_s.gsub("https://", "")
  end

  def self.short_url(url)
    display_url(url).split('/').first
  end
end
"#,
    )
    .expect("Failed to write formatter file");

    // File 2: User model that calls UrlFormatter
    let user_file = models_dir.join("user.rb");
    fs::write(
        &user_file,
        r#"
class User < ActiveRecord::Base
  def formatted_website
    UrlFormatter.display_url(self.website)
  end

  def short_website
    UrlFormatter.short_url(self.website)
  end
end
"#,
    )
    .expect("Failed to write user file");

    // Phase 1: Initial indexing (creates cache)
    let mut indexer1 = {
        let mut settings = Settings::default();
        settings.index_path = test_dir.join(".codanna");
        settings.workspace_root = Some(test_dir.to_path_buf());
        SimpleIndexer::with_settings(Arc::new(settings))
    };

    let stats1 = indexer1
        .index_directory_with_options(test_dir, false, false, false, None)
        .expect("Initial indexing failed");

    eprintln!("Phase 1 - Initial indexing: {} files, {} symbols",
              stats1.files_indexed, stats1.symbols_found);

    // Save the index to persist cache
    indexer1.save().expect("Failed to save index 1");

    // Verify relationships were collected in initial indexing
    let relationships1 = indexer1.list_relationships(None, None, None)
        .expect("Failed to list relationships after initial indexing");
    let rel_count1 = relationships1.len();

    eprintln!("Phase 1 - Relationships collected: {}", rel_count1);
    assert!(
        rel_count1 >= 4,
        "Expected at least 4 relationships (UrlFormatter.display_url, UrlFormatter.short_url, User->UrlFormatter.display_url, User->UrlFormatter.short_url), got {}",
        rel_count1
    );

    // Phase 2: Re-index without --force (simulates production CLI behavior)
    // This is the scenario that failed in production: files are cached,
    // so reindex_file_content is skipped, preventing relationship collection
    let mut indexer2 = {
        let mut settings = Settings::default();
        settings.index_path = test_dir.join(".codanna");
        settings.workspace_root = Some(test_dir.to_path_buf());
        SimpleIndexer::with_settings(Arc::new(settings))
    };

    let stats2 = indexer2
        .index_directory_with_options(test_dir, false, false, false, None)
        .expect("Cached re-indexing failed");

    eprintln!("Phase 2 - Cached re-indexing: {} files indexed", stats2.files_indexed);

    // CRITICAL: This test catches the production bug!
    // In the buggy version, cached files would skip relationship collection,
    // resulting in 0 relationships after re-indexing without --force.
    let relationships2 = indexer2.list_relationships(None, None, None)
        .expect("Failed to list relationships after cached re-indexing");
    let rel_count2 = relationships2.len();

    eprintln!("Phase 2 - Relationships after cache: {}", rel_count2);
    assert_eq!(
        rel_count2, rel_count1,
        "BUG: Cached re-indexing lost relationships! Expected {}, got {}. This indicates files were cached and skipped relationship collection.",
        rel_count1, rel_count2
    );

    // Phase 3: Re-index with --force (bypasses cache, should re-collect relationships)
    let mut indexer3 = {
        let mut settings = Settings::default();
        settings.index_path = test_dir.join(".codanna");
        settings.workspace_root = Some(test_dir.to_path_buf());
        SimpleIndexer::with_settings(Arc::new(settings))
    };

    let stats3 = indexer3
        .index_directory_with_options(test_dir, false, false, true, None)
        .expect("Forced re-indexing failed");

    eprintln!("Phase 3 - Forced re-indexing: {} files indexed", stats3.files_indexed);

    let relationships3 = indexer3.list_relationships(None, None, None)
        .expect("Failed to list relationships after forced re-indexing");
    let rel_count3 = relationships3.len();

    eprintln!("Phase 3 - Relationships after --force: {}", rel_count3);
    assert!(
        rel_count3 >= 4,
        "Forced re-indexing should re-collect relationships, expected at least 4, got {}",
        rel_count3
    );

    // Validate specific UrlFormatter relationships exist
    let has_display_url = relationships3.iter().any(|r| {
        r.to_name.contains("display_url")
    });
    let has_short_url = relationships3.iter().any(|r| {
        r.to_name.contains("short_url")
    });

    assert!(has_display_url, "Missing UrlFormatter.display_url relationship");
    assert!(has_short_url, "Missing UrlFormatter.short_url relationship");

    eprintln!("✅ All phases passed: caching behavior correctly preserves relationships");
}

/// Test that demonstrates the caching path vs fresh indexing path difference
#[test]
fn test_cache_vs_fresh_indexing_parity() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let test_dir = temp_dir.path();

    // Create a simple Ruby file with method calls
    let test_file = test_dir.join("test.rb");
    fs::write(
        &test_file,
        r#"
class Calculator
  def add(a, b)
    a + b
  end

  def multiply(a, b)
    a * b
  end
end

def use_calculator
  calc = Calculator.new
  result1 = calc.add(1, 2)
  result2 = calc.multiply(3, 4)
  result1 + result2
end
"#,
    )
    .expect("Failed to write test file");

    // Fresh indexing: relationships collected during initial parsing
    let mut indexer_fresh = {
        let mut settings = Settings::default();
        settings.index_path = test_dir.join(".codanna_fresh");
        settings.workspace_root = Some(test_dir.to_path_buf());
        SimpleIndexer::with_settings(Arc::new(settings))
    };

    indexer_fresh
        .index_directory_with_options(test_dir, false, false, false, None)
        .expect("Fresh indexing failed");

    let fresh_rels = indexer_fresh.list_relationships(None, None, None)
        .expect("Failed to list relationships from fresh indexing");

    // Cached indexing: load index, then re-index (should use cache)
    let mut indexer_cached = {
        let mut settings = Settings::default();
        settings.index_path = test_dir.join(".codanna_cached");
        settings.workspace_root = Some(test_dir.to_path_buf());
        SimpleIndexer::with_settings(Arc::new(settings))
    };

    // First pass: create cache
    indexer_cached
        .index_directory_with_options(test_dir, false, false, false, None)
        .expect("Initial cached indexing failed");
    indexer_cached.save().expect("Failed to save cached index");

    // Second pass: should hit cache
    let mut indexer_reopen = {
        let mut settings = Settings::default();
        settings.index_path = test_dir.join(".codanna_cached");
        settings.workspace_root = Some(test_dir.to_path_buf());
        SimpleIndexer::with_settings(Arc::new(settings))
    };

    indexer_reopen
        .index_directory_with_options(test_dir, false, false, false, None)
        .expect("Cached re-indexing failed");

    let cached_rels = indexer_reopen.list_relationships(None, None, None)
        .expect("Failed to list relationships from cached indexing");

    eprintln!("Fresh indexing: {} relationships", fresh_rels.len());
    eprintln!("Cached indexing: {} relationships", cached_rels.len());

    assert_eq!(
        fresh_rels.len(),
        cached_rels.len(),
        "Cache vs fresh indexing relationship count mismatch"
    );
}
