import {
  SlashCommandBuilder,
  EmbedBuilder,
  type ChatInputCommandInteraction,
} from "discord.js";
import { apiClient } from "../api-client";

export const spawn = {
  data: new SlashCommandBuilder()
    .setName("spawn")
    .setDescription("Spawn a new AI agent with a prompt")
    .addStringOption((option) =>
      option
        .setName("repo")
        .setDescription("GitHub repository in owner/repo format")
        .setRequired(true)
        .setMaxLength(200)
    )
    .addStringOption((option) =>
      option
        .setName("prompt")
        .setDescription("The prompt for the AI agent")
        .setRequired(true)
        .setMaxLength(4000)
    ),

  async execute(interaction: ChatInputCommandInteraction) {
    const repo = interaction.options.getString("repo", true);
    const prompt = interaction.options.getString("prompt", true);

    await interaction.deferReply();

    // Validate repository format
    const repoRegex = /^[a-zA-Z0-9._-]+\/[a-zA-Z0-9._-]+$/;
    if (!repoRegex.test(repo)) {
      await interaction.editReply({
        content: "Invalid repository format. Please use `owner/repo` format (e.g., `facebook/react`).",
      });
      return;
    }

    try {
      const task = await apiClient.createTask({
        prompt,
        repositories: [repo],
        source: "discord",
        user_id: interaction.user.id,
        guild_id: interaction.guildId ?? undefined,
      });

      const fields = [
        { name: "Task ID", value: `\`${task.id}\``, inline: true },
        { name: "Status", value: formatStatus(task.status), inline: true },
        { name: "Repository", value: `\`${repo}\``, inline: true },
        { name: "Prompt", value: truncate(prompt, 1024) },
      ];

      // Add SSH info if available
      if (task.ssh_command) {
        fields.push({
          name: "SSH Access",
          value: `\`${task.ssh_command}\``,
          inline: false,
        });
      }

      const embed = new EmbedBuilder()
        .setColor(0x5865f2)
        .setTitle("AI Agent Spawned")
        .setDescription(`Your AI agent is starting up...`)
        .addFields(...fields)
        .setFooter({ text: "Click the link below to view the agent" })
        .setTimestamp();

      let content = `**Open in browser:** ${task.web_url}`;
      if (task.ip_address) {
        content += `\n**SSH:** \`ssh root@${task.ip_address}\``;
      }

      await interaction.editReply({
        content,
        embeds: [embed],
      });
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown error occurred";
      await interaction.editReply({
        content: `Failed to spawn agent: ${message}`,
      });
    }
  },
};

function formatStatus(status: string): string {
  const statusEmojis: Record<string, string> = {
    pending: "⏳ Pending",
    starting: "🚀 Starting",
    running: "▶️ Running",
    suspended: "⏸️ Suspended",
    terminated: "⏹️ Terminated",
  };
  return statusEmojis[status] || status;
}

function truncate(str: string, maxLength: number): string {
  if (str.length <= maxLength) return str;
  return str.slice(0, maxLength - 3) + "...";
}
