import { Component, computed, effect, input, signal } from '@angular/core';
import { Member, Project } from '../users-api';

type ProjectPermission = 'edit_project' | 'start_deployment' | 'participate_in_deployment';

interface DraftRole {
  id: number;
  name: string;
  editRoles: boolean;
  projects: Record<number, Partial<Record<ProjectPermission, boolean>>>;
}

@Component({
  selector: 'app-role-editor',
  templateUrl: './role-editor.html',
  styleUrl: './role-editor.css',
})
export class RoleEditor {
  readonly organizationId = input.required<string>();
  readonly projects = input.required<Project[]>();
  readonly members = input.required<Member[]>();
  readonly projectsLoading = input(false);
  readonly projectsError = input('');
  readonly membersLoading = input(false);
  readonly membersError = input('');
  protected readonly roles = signal<DraftRole[]>([]);
  protected readonly selectedId = signal<number | null>(null);
  protected readonly newRoleName = signal('');
  protected readonly assignments = signal<Record<number, string>>({});
  protected readonly selectedRole = computed(() =>
    this.roles().find((role) => role.id === this.selectedId()));
  protected readonly projectPermissions: { key: ProjectPermission; label: string }[] = [
    { key: 'edit_project', label: 'Edit project' },
    { key: 'start_deployment', label: 'Start deployments' },
    { key: 'participate_in_deployment', label: 'Participate in deployments' },
  ];
  private nextId = 1;

  constructor() {
    effect(() => {
      this.organizationId();
      this.resetDraft();
    });
  }

  protected addRole() {
    const name = this.newRoleName().trim();
    if (!name || this.roles().some((role) => role.name.toLowerCase() === name.toLowerCase())) return;
    const role: DraftRole = { id: this.nextId++, name, editRoles: false, projects: {} };
    this.roles.update((roles) => [...roles, role]);
    this.selectedId.set(role.id);
    this.newRoleName.set('');
  }

  protected setOrganizationPermission(checked: boolean) {
    this.roles.update((roles) => roles.map((role) => role.id === this.selectedId()
      ? { ...role, editRoles: checked } : role));
  }

  protected setProjectPermission(projectId: number, permission: ProjectPermission, checked: boolean) {
    this.roles.update((roles) => roles.map((role) => role.id === this.selectedId()
      ? { ...role, projects: { ...role.projects,
          [projectId]: { ...role.projects[projectId], [permission]: checked } } }
      : role));
  }

  protected assignRole(memberId: number, role: string) {
    this.assignments.update((assignments) => ({ ...assignments, [memberId]: role }));
  }

  protected resetDraft() {
    this.roles.set([]);
    this.selectedId.set(null);
    this.newRoleName.set('');
    this.assignments.set({});
    this.nextId = 1;
  }
}
