//! Sophisticated confirmation workflow with risk assessment and context awareness.
//!
//! This module provides intelligent confirmation mechanisms that:
//! - Assess risk levels of operations
//! - Provide contextual information about operations
//! - Show previews of changes
//! - Handle persistent user preferences
//! - Support granular permission controls

use std::collections::HashMap;
use std::io::{self, Write};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Operation context for confirmation
#[derive(Debug, Clone)]
pub struct OperationContext {
    pub operation_type: OperationType,
    pub target_path: Option<String>,
    pub content_preview: Option<String>,
    pub risk_level: RiskLevel,
    pub alternatives: Vec<String>,
    pub estimated_impact: ImpactLevel,
}

/// Type of operation being performed
#[derive(Debug, Clone, PartialEq)]
pub enum OperationType {
    ReadFile,
    WriteFile,
    ModifyFile,
    DeleteFile,
    ExecuteCommand,
    CreateDirectory,
    MoveFile,
    CopyFile,
}

/// Risk level assessment
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Impact level estimation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImpactLevel {
    Minimal,
    Moderate,
    Significant,
    Severe,
}

/// User confirmation preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationPreferences {
    pub auto_approve_low_risk: bool,
    pub auto_approve_trusted_paths: Vec<String>,
    pub require_explicit_for_critical: bool,
    pub show_detailed_previews: bool,
    pub last_used_alternatives: HashMap<String, String>,
}

impl Default for ConfirmationPreferences {
    fn default() -> Self {
        Self {
            auto_approve_low_risk: true,
            auto_approve_trusted_paths: vec![],
            require_explicit_for_critical: true,
            show_detailed_previews: true,
            last_used_alternatives: HashMap::new(),
        }
    }
}

/// Enhanced confirmation system
pub struct ConfirmationSystem {
    preferences: ConfirmationPreferences,
    trust_cache: HashMap<String, bool>,
}

impl ConfirmationSystem {
    /// Create a new confirmation system
    pub fn new() -> Self {
        Self {
            preferences: ConfirmationPreferences::default(),
            trust_cache: HashMap::new(),
        }
    }

    /// Assess risk level for an operation
    pub fn assess_risk(&self, context: &OperationContext) -> RiskLevel {
        let mut risk_factors = 0;
        
        // Path-based risk factors
        if let Some(ref path) = context.target_path {
            let path_lower = path.to_lowercase();
            
            // System directories are high risk
            if path_lower.contains("program files") || 
               path_lower.contains("windows") || 
               path_lower.contains("/etc/") ||
               path_lower.contains("/system/") ||
               path_lower.contains("/usr/") {
                risk_factors += 3;
            }
            
            // User directories are medium risk
            else if path_lower.contains("desktop") || 
                  path_lower.contains("documents") || 
                  path_lower.contains("downloads") {
                risk_factors += 1;
            }
            
            // Temporary directories are low risk
            else if path_lower.contains("temp") || 
                  path_lower.contains("tmp") ||
                  path_lower.contains("/tmp/") {
                risk_factors -= 1;
            }
        }
        
        // Operation type risk factors
        match context.operation_type {
            OperationType::DeleteFile | OperationType::ExecuteCommand => risk_factors += 2,
            OperationType::ModifyFile | OperationType::MoveFile => risk_factors += 1,
            OperationType::WriteFile => risk_factors += 1,
            OperationType::ReadFile | OperationType::CreateDirectory | OperationType::CopyFile => {
                // These are generally lower risk
            }
        }
        
        // Impact level contribution
        match context.estimated_impact {
            ImpactLevel::Severe => risk_factors += 3,
            ImpactLevel::Significant => risk_factors += 2,
            ImpactLevel::Moderate => risk_factors += 1,
            ImpactLevel::Minimal => risk_factors -= 1,
        }
        
        // Convert risk factors to risk level
        match risk_factors {
            r if r <= 0 => RiskLevel::Low,
            r if r <= 2 => RiskLevel::Medium,
            r if r <= 4 => RiskLevel::High,
            _ => RiskLevel::Critical,
        }
    }

    /// Check if operation should be auto-approved
    pub fn should_auto_approve(&mut self, context: &OperationContext) -> bool {
        // Never auto-approve critical operations
        if context.risk_level == RiskLevel::Critical {
            return false;
        }
        
        // Auto-approve low-risk operations if preference is set
        if context.risk_level == RiskLevel::Low && self.preferences.auto_approve_low_risk {
            return true;
        }
        
        // Check trusted paths
        if let Some(ref path) = context.target_path {
            if self.is_path_trusted(path) {
                return true;
            }
        }
        
        false
    }

    /// Check if a path is in trusted locations
    fn is_path_trusted(&mut self, path: &str) -> bool {
        let path_lower = path.to_lowercase();
        
        // Check cache first
        if let Some(&trusted) = self.trust_cache.get(&path_lower) {
            return trusted;
        }
        
        // Check against trusted patterns
        let trusted = self.preferences.auto_approve_trusted_paths.iter()
            .any(|trusted_path| path_lower.contains(&trusted_path.to_lowercase()));
        
        // Cache the result
        self.trust_cache.insert(path_lower, trusted);
        trusted
    }

    /// Get detailed confirmation message
    fn get_confirmation_message(&self, context: &OperationContext) -> String {
        let mut message = String::new();
        
        // Operation header
        message.push_str(&format!("{} {} operation\n", 
            "⚠️".yellow(), 
            format_operation_type(&context.operation_type).cyan().bold()
        ));
        
        // Target information
        if let Some(ref path) = context.target_path {
            message.push_str(&format!("  {} {}\n", 
                "Target:".dimmed(), 
                path.white().bold()
            ));
        }
        
        // Risk assessment
        let risk_color = match context.risk_level {
            RiskLevel::Low => "🟢".green(),
            RiskLevel::Medium => "🟡".yellow(),
            RiskLevel::High => "🟠".yellow(),
            RiskLevel::Critical => "🔴".red(),
        };
        
        message.push_str(&format!("  {} Risk Level: {}\n", 
            risk_color, 
            format_risk_level(&context.risk_level).bold()
        ));
        
        // Impact assessment
        message.push_str(&format!("  {} Estimated Impact: {}\n", 
            "📊".blue(),
            format_impact_level(&context.estimated_impact)
        ));
        
        // Content preview for file operations
        if self.preferences.show_detailed_previews {
            if let Some(ref preview) = context.content_preview {
                if !preview.is_empty() {
                    message.push_str(&format!("\n  {} Content Preview:\n", "🔍".blue()));
                    let preview_lines: Vec<&str> = preview.lines().take(5).collect();
                    for line in preview_lines {
                        message.push_str(&format!("    {}\n", line.dimmed()));
                    }
                    if preview.lines().count() > 5 {
                        message.push_str(&format!("    ... ({} more lines)\n", 
                            preview.lines().count() - 5));
                    }
                }
            }
        }
        
        // Alternatives if available
        if !context.alternatives.is_empty() {
            message.push_str(&format!("\n  {} Safer alternatives available:\n", "💡".cyan()));
            for (i, alt) in context.alternatives.iter().enumerate() {
                message.push_str(&format!("    {}. {}\n", i + 1, alt));
            }
        }
        
        message.push_str(&format!("\n{} [y/N]: ", "Confirm this operation?".yellow()));
        message
    }

    /// Request user confirmation with context
    pub fn request_confirmation(&mut self, context: &OperationContext) -> Result<bool, anyhow::Error> {
        debug!("Requesting confirmation for operation: {:?}", context.operation_type);
        
        // Auto-approve if appropriate
        if self.should_auto_approve(context) {
            println!("{} {} operation auto-approved", 
                "✅".green(), 
                format_operation_type(&context.operation_type)
            );
            return Ok(true);
        }
        
        // For critical operations, always require explicit confirmation
        if context.risk_level == RiskLevel::Critical && self.preferences.require_explicit_for_critical {
            println!("{}", self.get_confirmation_message(context));
            return self.get_user_input();
        }
        
        // Show detailed confirmation for high/medium risk
        match context.risk_level {
            RiskLevel::High | RiskLevel::Critical => {
                println!("{}", self.get_confirmation_message(context));
                self.get_user_input()
            }
            RiskLevel::Medium => {
                // Brief confirmation for medium risk
                println!("{} {} operation ({} risk)", 
                    "⚠️".yellow(),
                    format_operation_type(&context.operation_type),
                    format_risk_level(&context.risk_level).yellow()
                );
                if let Some(ref path) = context.target_path {
                    println!("  Target: {}", path);
                }
                print!("{} [y/N]: ", "Confirm?".yellow());
                io::stdout().flush()?;
                self.get_user_input()
            }
            RiskLevel::Low => {
                // Very brief confirmation or auto-approve based on settings
                if self.preferences.auto_approve_low_risk {
                    Ok(true)
                } else {
                    println!("{} {} operation", 
                        "ℹ️".blue(), 
                        format_operation_type(&context.operation_type)
                    );
                    print!("{} [Y/n]: ", "Proceed?".blue());
                    io::stdout().flush()?;
                    
                    let mut input = String::new();
                    io::stdin().read_line(&mut input)?;
                    let response = input.trim().to_lowercase();
                    Ok(response.is_empty() || response == "y" || response == "yes")
                }
            }
        }
    }

    /// Get user input for confirmation
    fn get_user_input(&self) -> Result<bool, anyhow::Error> {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let response = input.trim().to_lowercase();
        Ok(response == "y" || response == "yes")
    }

    /// Add a path to trusted locations
    pub fn add_trusted_path(&mut self, path: &str) {
        let normalized_path = path.to_lowercase();
        if !self.preferences.auto_approve_trusted_paths.contains(&normalized_path) {
            self.preferences.auto_approve_trusted_paths.push(normalized_path);
            // Clear cache since trust status may have changed
            self.trust_cache.clear();
        }
    }

    /// Remove a path from trusted locations
    pub fn remove_trusted_path(&mut self, path: &str) {
        let normalized_path = path.to_lowercase();
        self.preferences.auto_approve_trusted_paths.retain(|p| 
            !p.to_lowercase().contains(&normalized_path)
        );
        // Clear cache since trust status may have changed
        self.trust_cache.clear();
    }

    /// Update preferences
    pub fn update_preferences<F>(&mut self, updater: F) 
    where F: FnOnce(&mut ConfirmationPreferences) {
        updater(&mut self.preferences);
    }

    /// Get current preferences
    pub fn get_preferences(&self) -> &ConfirmationPreferences {
        &self.preferences
    }
}

/// Format operation type for display
fn format_operation_type(op_type: &OperationType) -> String {
    match op_type {
        OperationType::ReadFile => "Read File".to_string(),
        OperationType::WriteFile => "Write File".to_string(),
        OperationType::ModifyFile => "Modify File".to_string(),
        OperationType::DeleteFile => "Delete File".to_string(),
        OperationType::ExecuteCommand => "Execute Command".to_string(),
        OperationType::CreateDirectory => "Create Directory".to_string(),
        OperationType::MoveFile => "Move File".to_string(),
        OperationType::CopyFile => "Copy File".to_string(),
    }
}

/// Format risk level for display
fn format_risk_level(risk: &RiskLevel) -> String {
    match risk {
        RiskLevel::Low => "Low".to_string(),
        RiskLevel::Medium => "Medium".to_string(),
        RiskLevel::High => "High".to_string(),
        RiskLevel::Critical => "Critical".to_string(),
    }
}

/// Format impact level for display
fn format_impact_level(impact: &ImpactLevel) -> String {
    match impact {
        ImpactLevel::Minimal => "Minimal".to_string(),
        ImpactLevel::Moderate => "Moderate".to_string(),
        ImpactLevel::Significant => "Significant".to_string(),
        ImpactLevel::Severe => "Severe".to_string(),
    }
}

/// Helper function to create operation context
pub fn create_operation_context(
    operation_type: OperationType,
    target_path: Option<&str>,
    content_preview: Option<&str>,
    alternatives: Vec<String>,
) -> OperationContext {
    let context = OperationContext {
        operation_type,
        target_path: target_path.map(|s| s.to_string()),
        content_preview: content_preview.map(|s| s.to_string()),
        risk_level: RiskLevel::Low, // Will be assessed separately
        alternatives,
        estimated_impact: ImpactLevel::Minimal,
    };
    
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_assessment() {
        let confirmation_system = ConfirmationSystem::new();
        
        let context = OperationContext {
            operation_type: OperationType::DeleteFile,
            target_path: Some("/etc/critical.conf".to_string()),
            content_preview: None,
            risk_level: RiskLevel::Low, // Will be reassessed
            alternatives: vec![],
            estimated_impact: ImpactLevel::Significant,
        };
        
        let assessed_risk = confirmation_system.assess_risk(&context);
        assert!(assessed_risk >= RiskLevel::High);
    }

    #[test]
    fn test_trusted_paths() {
        let mut confirmation_system = ConfirmationSystem::new();
        confirmation_system.add_trusted_path("./safe_project");
        
        let context = OperationContext {
            operation_type: OperationType::WriteFile,
            target_path: Some("./safe_project/test.txt".to_string()),
            content_preview: None,
            risk_level: RiskLevel::Low,
            alternatives: vec![],
            estimated_impact: ImpactLevel::Minimal,
        };
        
        assert!(confirmation_system.is_path_trusted("./safe_project/test.txt"));
        assert!(confirmation_system.should_auto_approve(&context));
    }
}