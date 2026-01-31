// SM-SF-2: the problems line of a coding task. `problems_unchecked` absent reads as false, and the
// words "No new problems" belong to one row only: a task whose files were all checked and found clean.
//
// spec: ordin-desktop/components/ScreensModule#SM-SF-2
import type { Problem, ReviewTask } from '../../service-api/types.gen';

export type ProblemsRow = 'none' | 'count' | 'unchecked' | 'count_and_unchecked';
export type ProblemsIcon = 'tick' | 'warning' | 'question';

export interface ProblemsLine {
  row: ProblemsRow;
  icon: ProblemsIcon;
  /** The message ids drawn, in order. */
  messageIds: string[];
  count: number;
  problems: Problem[];
}

export function problemsLine(review: Pick<ReviewTask, 'task' | 'new_problems'>): ProblemsLine {
  const unchecked = review.task.problems_unchecked === true;
  const count = review.task.new_problems;
  const problems = review.new_problems;
  if (!unchecked && count === 0) return { row: 'none', icon: 'tick', messageIds: ['changes-problems-none'], count, problems };
  if (!unchecked) return { row: 'count', icon: 'warning', messageIds: ['changes-problems-count'], count, problems };
  if (count === 0) return { row: 'unchecked', icon: 'question', messageIds: ['changes-problems-unchecked'], count, problems };
  return { row: 'count_and_unchecked', icon: 'warning', messageIds: ['changes-problems-count', 'changes-problems-unchecked-also'], count, problems };
}
