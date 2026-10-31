import type { Messages } from "..";

export const common: Messages["common"] = {
  cancel: "Cancel",
  close: "Close",
  back: "Back",
  remove: "Remove",
  noDate: "no date",
  noDateTitle: "No date",
  months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
  files: (n: string) => `${n} files`,
};
