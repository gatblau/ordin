// After the last journey: the program and the stand-ins stop. The data root and the run folder stay for
// whoever reads the report.
import { stopOrdin } from './ordin';
import { stopStandIns } from './stand-ins';

export default async function globalTeardown(): Promise<void> {
  await stopOrdin();
  stopStandIns();
}
