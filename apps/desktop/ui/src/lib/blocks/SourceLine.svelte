<script lang="ts">
  // "Where these numbers come from": the part's source in one line (ADR-067). When an agent worked the
  // numbers out, the line says so.
  import { formatDate, formatList, formatTime } from '../format/intl';
  import { t } from '../i18n/fluent';
  import type { ReportPartSource } from '../service-api/types.gen';
  import { refName } from './refs';

  interface Props {
    source: ReportPartSource;
  }
  let { source }: Props = $props();

  const namesOf = (refs: ReadonlyArray<Parameters<typeof refName>[0]>): string =>
    formatList(refs.map((r) => refName(r) ?? t('block-file-unnamed')));
  const line = $derived.by(() => {
    switch (source.kind) {
      case 'documents':
        return t('block-source-documents', {
          documents: formatList(source.documents.map((d) => t('block-source-document', { name: refName(d.item) ?? t('block-file-unnamed'), version: d.version }))),
        });
      case 'company_system':
        return t('block-source-company-system', { system: source.system, date: formatDate(source.read_at), time: formatTime(source.read_at) });
      case 'earlier_result':
        return t('block-source-earlier-result');
      case 'worked_out':
        return t('block-source-worked-out', { agent: source.by, files: namesOf(source.from), count: source.from.length });
    }
  });
</script>

<p class="source"><span class="source-label">{t('report-sources')}</span> <span class="source-text">{line}</span></p>

<style>
  .source {
    margin: 0;
    color: var(--color-text-muted);
    font-size: 12px;
  }
  .source-label {
    font-weight: 600;
  }
</style>
