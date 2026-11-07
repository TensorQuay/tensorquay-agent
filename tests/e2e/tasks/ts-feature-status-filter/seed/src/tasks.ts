export type Status = "open" | "done";

export interface Task {
  id: number;
  title: string;
  status: Status;
  tags: string[];
}

export interface ListOptions {
  /** Keep only tasks carrying this tag. */
  tag?: string;
}

/** Returns the matching tasks ordered by id. */
export function listTasks(tasks: Task[], options: ListOptions = {}): Task[] {
  return tasks
    .filter((task) => options.tag === undefined || task.tags.includes(options.tag))
    .sort((a, b) => a.id - b.id);
}
