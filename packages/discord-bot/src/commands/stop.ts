import {
  SlashCommandBuilder,
  EmbedBuilder,
  type ChatInputCommandInteraction,
} from "discord.js";
import { apiClient } from "../api-client";

export const stop = {
  data: new SlashCommandBuilder()
    .setName("stop")
    .setDescription("Stop and terminate an AI agent")
    .addStringOption((option) =>
      option
        .setName("task_id")
        .setDescription("The task ID to stop")
        .setRequired(true)
    ),

  async execute(interaction: ChatInputCommandInteraction) {
    const taskId = interaction.options.getString("task_id", true);

    await interaction.deferReply();

    try {
      await apiClient.deleteTask(taskId);

      const embed = new EmbedBuilder()
        .setColor(0xff0000)
        .setTitle("Agent Terminated")
        .setDescription(
          "The AI agent has been terminated and all resources released."
        )
        .addFields({ name: "Task ID", value: `\`${taskId}\`` })
        .setTimestamp();

      await interaction.editReply({
        embeds: [embed],
      });
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown error occurred";
      await interaction.editReply({
        content: `Failed to stop agent: ${message}`,
      });
    }
  },
};
