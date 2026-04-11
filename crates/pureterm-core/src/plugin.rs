use std::any::Any;

/// Trait for PureTerm plugins.
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str { "" }

    /// Called when the plugin is loaded.
    fn on_load(&self, _context: &mut PluginContext) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called when the plugin is unloaded.
    fn on_unload(&self) {}

    /// Called to register services.
    fn register_services(&self, _registry: &mut ServiceRegistry) {}
}

/// Context provided to plugins during initialization.
pub struct PluginContext {
    // Will be expanded as needed
}

/// Registry for plugin-provided services.
pub struct ServiceRegistry {
    commands: Vec<CommandDescriptor>,
}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn register_command(&mut self, command: CommandDescriptor) {
        self.commands.push(command);
    }

    pub fn commands(&self) -> &[CommandDescriptor] {
        &self.commands
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct CommandDescriptor {
    pub id: String,
    pub label: String,
    pub category: String,
}

pub struct PluginManager {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn load_plugin(&mut self, plugin: Box<dyn Plugin>) -> anyhow::Result<()> {
        log::info!("Loading plugin: {} v{}", plugin.name(), plugin.version());
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn unload_plugin(&mut self, name: &str) {
        if let Some(pos) = self.plugins.iter().position(|p| p.name() == name) {
            let plugin = self.plugins.remove(pos);
            plugin.on_unload();
            log::info!("Unloaded plugin: {}", name);
        }
    }

    pub fn initialize_all(&mut self) {
        let mut registry = ServiceRegistry::new();
        for plugin in &self.plugins {
            plugin.register_services(&mut registry);
        }
    }

    pub fn plugins(&self) -> &[Box<dyn Plugin>] {
        &self.plugins
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}
