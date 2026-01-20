import {
  SlashCommandBuilder,
  EmbedBuilder,
  type ChatInputCommandInteraction,
} from "discord.js";
import { apiClient } from "../api-client";

export const resume = {
  data: new SlashCommandBuilder()
    .setName("resume")
    .setDescription("Resume a suspended AI agent")
    .addStringOption((option) =>
      option
        .setName("task_id")
        .setDescription("The task ID to resume")
        .setRequired(true)
    ),

  async execute(interaction: ChatInputCommandInteraction) {
    const taskId = interaction.options.getString("task_id", true);

    await interaction.deferReply();

    try {
      const task = await apiClient.resumeTask(taskId);

      const embed = new EmbedBuilder()
        .setColor(0x00ff00)
        .setTitle("Agent Resumed")
        .setDescription("The AI agent has been resumed.")
        .addFields(
          { name: "Task ID", value: `\`${task.id}\``, inline: true },
          { name: "Status", value: formatStatus(task.status), inline: true }
        )
        .setTimestamp();

      await interaction.editReply({
        content: `**Open in browser:** ${task.web_url}`,
        embeds: [embed],
      });
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown error occurred";
      await interaction.editReply({
        content: `Failed to resume agent: ${message}`,
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
