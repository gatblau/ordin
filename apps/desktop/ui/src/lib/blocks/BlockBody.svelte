<script lang="ts">
  // The content of one block, by kind. A kind this version does not know is not drawn.
  import type { BlockViewProps } from '../shell/screens';
  import AgentLibrary from './kinds/AgentLibrary.svelte';
  import Access from './kinds/Access.svelte';
  import AgentProposal from './kinds/AgentProposal.svelte';
  import Changes from './kinds/Changes.svelte';
  import Chart from './kinds/Chart.svelte';
  import CrewPlan from './kinds/CrewPlan.svelte';
  import Document from './kinds/Document.svelte';
  import Documents from './kinds/Documents.svelte';
  import Email from './kinds/Email.svelte';
  import EmailList from './kinds/EmailList.svelte';
  import Form from './kinds/Form.svelte';
  import KeyFigures from './kinds/KeyFigures.svelte';
  import Knowledge from './kinds/Knowledge.svelte';
  import Memory from './kinds/Memory.svelte';
  import Message from './kinds/Message.svelte';
  import PlanProgress from './kinds/PlanProgress.svelte';
  import Preview from './kinds/Preview.svelte';
  import Question from './kinds/Question.svelte';
  import Report from './kinds/Report.svelte';
  import Result from './kinds/Result.svelte';
  import SlackList from './kinds/SlackList.svelte';
  import Table from './kinds/Table.svelte';
  import TelegramList from './kinds/TelegramList.svelte';

  let { backend, workspaceId, blockId, block }: BlockViewProps = $props();
</script>

{#if block.kind === 'message'}
  <Message {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'question'}
  <Question {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'form'}
  <Form {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'documents'}
  <Documents {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'preview'}
  <Preview {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'document'}
  <Document {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'changes'}
  <Changes {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'plan_progress'}
  <PlanProgress {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'table'}
  <Table {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'chart'}
  <Chart {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'key_figures'}
  <KeyFigures {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'report'}
  <Report {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'email_list'}
  <EmailList {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'email'}
  <Email {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'telegram_list'}
  <TelegramList {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'slack_list'}
  <SlackList {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'result'}
  <Result {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'access'}
  <Access {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'agent_proposal'}
  <AgentProposal {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'agent_library'}
  <AgentLibrary {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'crew_plan'}
  <CrewPlan {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'memory'}
  <Memory {backend} {workspaceId} {blockId} {block} />
{:else if block.kind === 'knowledge'}
  <Knowledge {backend} {workspaceId} {blockId} {block} />
{/if}
