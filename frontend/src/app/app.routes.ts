import { inject } from '@angular/core';
import { CanActivateFn, Router, Routes } from '@angular/router';
import { ConnectorsPage } from './connectors/connectors';
import { Home } from './home/home';
import { Layout } from './layout/layout';
import { Login } from './login/login';
import { OrganizationPage } from './organization/organization';
import { RolesPage } from './organization/roles-page';
import { ProjectPage } from './project/project';
import { UsersApi } from './users-api';

const authenticated: CanActivateFn = () =>
  inject(UsersApi).token() ? true : inject(Router).createUrlTree(['/login']);

export const routes: Routes = [
  { path: '', pathMatch: 'full', redirectTo: 'login' },
  { path: 'login', component: Login },
  {
    path: '',
    component: Layout,
    canActivate: [authenticated],
    canActivateChild: [authenticated],
    children: [
      { path: 'home', component: Home },
      { path: 'connectors', component: ConnectorsPage },
      { path: 'organizations/:id', component: OrganizationPage },
      { path: 'organizations/:id/roles', component: RolesPage },
      { path: 'organizations/:orgId/projects/:id', component: ProjectPage, canDeactivate: [(component: ProjectPage) => component.canLeave()] },
    ],
  },
  { path: '**', redirectTo: 'login' },
];
