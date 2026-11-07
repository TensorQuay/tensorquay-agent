"use client";

import { useQuery } from "@tanstack/react-query";
import Link from "next/link";

import { api } from "../api/client";
import { modelName } from "../conversations/model-name";
import { RelativeTime } from "../conversations/RelativeTime";
import { useModels } from "../conversations/use-models";
import { useTemplates } from "../conversations/use-templates";
import { useT } from "../i18n/I18nProvider";
import type { User } from "../protocol/generated/api";
import { Button } from "../ui/Button";
import { Skeleton } from "../ui/Skeleton";
import { TableScroll } from "../ui/TableScroll";

import { DateLabel } from "./DateLabel";
import { allTasks } from "./team-queries";

/** One person's tasks for an administrator (task 067 Q4): metadata rows only, each opening the private view. */
export function UserTasks({ me, id }: { me: User; id: string }) {
  const t = useT();
  if (me.role !== "admin")
    return <p className="muted">{t("users.adminsOnly")}</p>;
  return <PersonTasks id={id} />;
}

function PersonTasks({ id }: { id: string }) {
  const t = useT();
  const users = useQuery({
    queryKey: ["users"],
    queryFn: ({ signal }) => api.users(signal),
  });
  const tasks = useQuery({
    queryKey: ["all-tasks"],
    queryFn: ({ signal }) => allTasks(signal),
  });
  // Environments and models as people read them everywhere else (WEB-U-172); an id the lists no longer have stands.
  const templates = useTemplates();
  const models = useModels();
  const environmentName = (name: string) =>
    templates.choices.find((choice) => choice.id === name)?.label;
  const routeName = (name: string) => {
    const route = models.data?.items.find((item) => item.name === name);
    return route ? modelName(route) : undefined;
  };
  // The names are waited for, so no id flashes before them; a list that fails leaves the ids (WEB-U-172).
  const naming = templates.isPending || models.isPending;
  const date = (value: string) =>
    new Intl.DateTimeFormat(t.locale, { dateStyle: "medium" }).format(
      new Date(value),
    );
  const person = users.data?.items.find((account) => account.id === id);
  const owned = tasks.data?.filter((task) => task.owner_id === id) ?? [];
  const failed = users.isError || tasks.isError;
  return (
    <div className="settings-section stack">
      <Link href="/settings/people" className="back-link type-small">
        {t("users.back")}
      </Link>
      {failed ? (
        <div className="stack">
          <p role="alert">{t("users.loadFailed")}</p>
          <Button
            variant="secondary"
            onClick={() => {
              void users.refetch();
              void tasks.refetch();
            }}
          >
            {t("common.tryAgain")}
          </Button>
        </div>
      ) : users.isSuccess && !person ? (
        // An id that isn't one of this installation's people (deleted, or mistyped) reads as gone (WEB-U-357).
        <p className="muted">{t("users.notFound")}</p>
      ) : !person || !tasks.data ? (
        <Skeleton label={t("skeleton.loading")} />
      ) : (
        <>
          <h2 className="type-heading" id="person-tasks">
            {t("users.tasksOf", { email: person.email })}
          </h2>
          <p className="muted">{t("users.tasksNote")}</p>
          {owned.length === 0 ? (
            <p className="muted">{t("users.noTasks")}</p>
          ) : naming ? (
            <Skeleton label={t("skeleton.loading")} />
          ) : (
            <TableScroll labelledBy="person-tasks">
              <table className="data-table" aria-labelledby="person-tasks">
                <thead className="type-label">
                  <tr>
                    <th scope="col">{t("private.created")}</th>
                    <th scope="col">{t("private.lastActivity")}</th>
                    <th scope="col">{t("private.environment")}</th>
                    <th scope="col">{t("private.model")}</th>
                    <th scope="col">{t("private.state")}</th>
                  </tr>
                </thead>
                <tbody>
                  {owned.map((task) => (
                    <tr key={task.id}>
                      {/* No title to show (SEC-005): the created date opens the task's details. */}
                      <td>
                        <Link
                          href={`/c/${encodeURIComponent(task.id)}`}
                          aria-label={t("users.taskLink", {
                            date: date(task.created_at),
                            environment:
                              environmentName(task.template) ?? task.template,
                            model:
                              routeName(task.model_route) ?? task.model_route,
                          })}
                        >
                          <DateLabel value={task.created_at} />
                        </Link>
                      </td>
                      <td>
                        <RelativeTime value={task.last_activity_at} />
                      </td>
                      <td>
                        <Named name={environmentName(task.template)}>
                          {task.template}
                        </Named>
                      </td>
                      <td>
                        <Named name={routeName(task.model_route)}>
                          {task.model_route}
                        </Named>
                      </td>
                      <td>{t("private.stateName", { state: task.state })}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </TableScroll>
          )}
        </>
      )}
    </div>
  );
}

/** A display name, or the id set as one while no list names it. */
function Named({
  name,
  children,
}: {
  name: string | undefined;
  children: string;
}) {
  return name ? <>{name}</> : <code className="type-code">{children}</code>;
}
