export interface UserCreateV2 {
  email: string;
  displayName: string;
}

export interface UserV2 extends UserCreateV2 {
  id: string;
}

export const route = {
  method: "POST",
  path: "/v2/users",
  operationId: "createUserV2",
  handler: "createUserV2",
} as const;

export async function createUserV2(input: UserCreateV2): Promise<UserV2> {
  return {
    id: "user_1",
    email: input.email,
    displayName: input.displayName,
  };
}
