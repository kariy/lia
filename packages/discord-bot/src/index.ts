import {
  Client,
  GatewayIntentBits,
  Events,
  Collection,
  type ChatInputCommandInteraction,
} from "discord.js";
import { commands } from "./commands";
import { config } from "./config";

// Create Discord client
const client = new Client({
  intents: [GatewayIntentBits.Guilds],
});

// Store commands in client
const commandCollection = new Collection<
  string,
  {
    data: { name: string };
    execute: (interaction: ChatInputCommandInteraction) => Promise<void>;
  }
>();

for (const command of commands) {
  commandCollection.set(command.data.name, command);
}

// Handle ready event
client.once(Events.ClientReady, (readyClient) => {
  console.log(`Logged in as ${readyClient.user.tag}`);
});

// Handle interactions
client.on(Events.InteractionCreate, async (interaction) => {
  if (!interaction.isChatInputCommand()) return;

  const command = commandCollection.get(interaction.commandName);
  if (!command) {
    console.error(`Command ${interaction.commandName} not found`);
    return;
  }

  try {
    await command.execute(interaction);
  } catch (error) {
    console.error(`Error executing command ${interaction.commandName}:`, error);

    const errorMessage = "An error occurred while executing this command.";
    if (interaction.replied || interaction.deferred) {
      await interaction.followUp({ content: errorMessage, ephemeral: true });
    } else {
      await interaction.reply({ content: errorMessage, ephemeral: true });
    }
  }
});

// Login
client.login(config.discordToken);
