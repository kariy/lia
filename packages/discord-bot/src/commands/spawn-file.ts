import {
  SlashCommandBuilder,
  EmbedBuilder,
  type ChatInputCommandInteraction,
} from "discord.js";
import { apiClient } from "../api-client";

export const spawnFile = {
  data: new SlashCommandBuilder()
    .setName("spawn-file")
    .setDescription("Spawn an AI agent with a file attachment")
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
    )
    .addAttachmentOption((option) =>
      option
        .setName("file")
        .setDescription("A file to provide as context")
        .setRequired(true)
    ),

  async execute(interaction: ChatInputCommandInteraction) {
    const repo = interaction.options.getString("repo", true);
    const prompt = interaction.options.getString("prompt", true);
    const attachment = interaction.options.getAttachment("file", true);

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
      // Fetch file content
      const response = await fetch(attachment.url);
      if (!response.ok) {
        throw new Error("Failed to fetch attachment");
      }

      const content = await response.text();

      // Validate file size (max 10MB)
      if (content.length > 10 * 1024 * 1024) {
        await interaction.editReply({
          content: "File is too large. Maximum size is 10MB.",
        });
        return;
      }

      const task = await apiClient.createTask({
        prompt,
        repositories: [repo],
        source: "discord",
        user_id: interaction.user.id,
        guild_id: interaction.guildId ?? undefined,
        files: [
          {
            name: attachment.name,
            content,
          },
        ],
      });

      const embed = new EmbedBuilder()
        .setColor(0x5865f2)
        .setTitle("AI Agent Spawned with File")
        .setDescription(`Your AI agent is starting up...`)
        .addFields(
          { name: "Task ID", value: `\`${task.id}\``, inline: true },
          { name: "Status", value: formatStatus(task.status), inline: true },
          { name: "Repository", value: `\`${repo}\``, inline: true },
          { name: "File", value: `\`${attachment.name}\``, inline: true },
          { name: "Prompt", value: truncate(prompt, 1024) }
        )
        .setFooter({ text: "Click the link below to view the agent" })
        .setTimestamp();

      await interaction.editReply({
        content: `**Open in browser:** ${task.web_url}`,
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
