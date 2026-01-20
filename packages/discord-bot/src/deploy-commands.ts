import { REST, Routes } from "discord.js";
import { commands } from "./commands";
import { config } from "./config";

const rest = new REST().setToken(config.discordToken);

async function deployCommands() {
  try {
    console.log(`Deploying ${commands.length} commands...`);

    const commandData = commands.map((command) => command.data.toJSON());

    // Deploy globally
    await rest.put(Routes.applicationCommands(config.discordClientId), {
      body: commandData,
    });

    console.log("Commands deployed successfully!");
  } catch (error) {
    console.error("Failed to deploy commands:", error);
    process.exit(1);
  }
}

deployCommands();
